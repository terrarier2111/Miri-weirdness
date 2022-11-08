use aligned::{Aligned, A4};
use std::alloc::{alloc, dealloc, Layout, LayoutError};
use std::arch::asm;
use std::mem::{align_of, needs_drop, size_of, transmute, ManuallyDrop, MaybeUninit};
use std::ops::{Deref, DerefMut};
use std::ptr::{addr_of, addr_of_mut, null_mut, NonNull, null};
use std::sync::atomic::{fence, AtomicPtr, AtomicU8, Ordering, AtomicUsize};
use std::sync::Arc;
use std::time::Duration;
use std::{mem, ptr, thread};
use std::borrow::Borrow;
use std::cell::UnsafeCell;
use std::fmt::{Debug, Display, Formatter};
use std::intrinsics::unlikely;
use std::marker::PhantomData;
use std::pin::Pin;
use std::process::abort;
use thread_local::ThreadLocal;

// disable MIRI SB(stacked borrows) checks

// this is better for cases where we DON'T care much about removal of nodes during traversal:
// https://www.codeproject.com/Articles/723555/A-Lock-Free-Doubly-Linked-List
// this is better if we DO:
// https://scholar.google.com/citations?view_op=view_citation&hl=de&user=RJmBj1wAAAAJ&citation_for_view=RJmBj1wAAAAJ:UebtZRa9Y70C

// FIXME: NOTE THAT: &mut T can be converted into *mut T by using .into() on the mutable reference!

// FIXME: add guard to nodes in Unbound mode to remove them once all of the references to them get dropped
// FIXME: employ reference counting on the nodes in order for them to be dropped correctly
#[derive(Clone)]
pub struct AtomicDoublyLinkedList<T, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    header_node: Aligned<A4, Arc<RawNode<T, NODE_KIND>>>,
    // in the header node itself, the left field points to the `header_node` field of the list itself, so we don't have to maintain a reference count
    tail_node: Aligned<A4, Arc<RawNode<T, NODE_KIND>>>,
    // in the tail node itself, the right field points to the `tail_node` field of the list itself, so we don't have to maintain a reference count
}

impl<T, const NODE_KIND: NodeKind> AtomicDoublyLinkedList<T, NODE_KIND> {
    const ENDS_ORDERING: Ordering = Ordering::SeqCst;

    pub fn new() -> Arc<Self> {
        if size_of::<Node<T, NODE_KIND, false>>() != size_of::<Node<T, NODE_KIND, true>>() {
            // check if the sizes for false and true differ
            unreachable!("Node size can't be evaluated statically!");
        }
        if align_of::<Node<T, NODE_KIND, false>>() != align_of::<Node<T, NODE_KIND, true>>() {
            // check if the alignments for false and true differ
            unreachable!("Node alignment can't be evaluated statically!");
        }
        if size_of::<RawNode<T, NODE_KIND, false>>() != size_of::<RawNode<T, NODE_KIND, true>>() {
            // check if the sizes for false and true differ
            unreachable!("Node size can't be evaluated statically!");
        }
        if align_of::<RawNode<T, NODE_KIND, false>>() != align_of::<RawNode<T, NODE_KIND, true>>() {
            // check if the alignments for false and true differ
            unreachable!("Node alignment can't be evaluated statically!");
        }
        if size_of::<Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, false>>>>()
            != size_of::<Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, true>>>>()
        {
            unreachable!("Arc's size can't be evaluated statically!");
        }
        if align_of::<Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, false>>>>()
            != align_of::<Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, true>>>>()
        {
            unreachable!("Arc's alignment can't be evaluated statically!");
        }
        if align_of::<Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND>>>>() < 4 {
            unreachable!("Arc's alignment isn't sufficient!");
        }
        let ret = Arc::new(Self {
            header_node: Aligned(Arc::new(Aligned(AtomicDoublyLinkedListNode {
                val: MaybeUninit::uninit(),
                left: Link::invalid(),
                right: Link::invalid(),
            }))),
            tail_node: Aligned(Arc::new(Aligned(AtomicDoublyLinkedListNode {
                val: MaybeUninit::uninit(),
                left: Link::invalid(),
                right: Link::invalid(),
            }))),
        });

        // SAFETY: we know that there are no other threads setting modifying
        // these nodes and thus they will automatically be correct
        unsafe {
            println!("tail addr: {:?}, mod 8: {}", ret.tail_addr(), ret.tail_addr().expose_addr() % 8);
            // create_ref(ret.tail_addr());
            ret.header_node.right.set_unsafe/*::<false>*/(ret.tail_addr());
        }
        unsafe {
            // create_ref(ret.header_addr());
            ret.tail_node.left.set_unsafe/*::<false>*/(ret.header_addr());
        }

        ret
    }

    /*
    pub fn new<const NODES: NodeKind>() -> AtomicDoublyLinkedList<T, NODES> {
        AtomicDoublyLinkedList {
            header_node: Default::default(),
            tail_node: Default::default(),
        }
    }*/

    #[inline]
    fn header_addr(&self) -> NodePtr<T, NODE_KIND> {
        Arc::as_ptr(&self.header_node)
    }

    #[inline]
    fn tail_addr(&self) -> NodePtr<T, NODE_KIND> {
        Arc::as_ptr(&self.tail_node)
    }

    pub fn push_head(&self, val: T) -> Node<T, NODE_KIND> {
        self.header_node.add_after(val)
    }

    pub fn push_tail(&self, val: T) -> Node<T, NODE_KIND> {
        self.tail_node.add_before(val)
    }

    pub fn remove_head(&self) -> Option<Node<T, { NodeKind::Bound }, true>> {
        loop {
            if self.is_empty() {
                return None;
            }
            if NODE_KIND == NodeKind::Bound {
                let head = self
                    .header_node
                    .right
                    .get_full();
                let head = head
                    .get_ptr()
                    .cast::<RawNode<T, { NodeKind::Bound }>>();
                let head = ManuallyDrop::new(Aligned(unsafe { Arc::from_raw(head) }));
                if let Some(val) = head.remove() {
                    println!("removed head!");
                    println!("rem end head: {:?}", self.header_node.right.get().raw_ptr());
                    println!("rem end tail: {:?}", self.tail_node.left.get().raw_ptr());
                    println!("ret node: {:?}", Arc::as_ptr(&val)); // FIXME: this is the same as rem end head and rem end tail
                    return Some(val);
                }
            } else {
                let head = self
                    .header_node
                    .right
                    .get_full();
                let head = head
                    .get_ptr()
                    .cast::<RawNode<T, { NodeKind::Unbound }>>();
                // increase the ref count because we want to return a reference to the node and thus we have to create a reference out of thin air
                unsafe { create_ref(head); }
                let head = Aligned(unsafe { Arc::from_raw(head) });
                if let Some(val) = head.remove() {
                    println!("removed head!");
                    println!("rem end head: {:?}", self.header_node.right.get().raw_ptr());
                    println!("rem end tail: {:?}", self.tail_node.left.get().raw_ptr());
                    return Some(val);
                }
            }
        }
    }

    pub fn remove_tail(&self) -> Option<Node<T, { NodeKind::Bound }, true>> {
        loop {
            if self.is_empty() {
                return None;
            }
            if NODE_KIND == NodeKind::Bound {
                let tail = self
                    .tail_node
                    .left
                    .get_full()
                    .get_ptr()
                    .cast::<RawNode<T, { NodeKind::Bound }>>();
                let tail = ManuallyDrop::new(Aligned(unsafe { Arc::from_raw(tail) }));
                if let Some(val) = tail.remove() {
                    println!("removed tail!");
                    println!("rem end head: {:?}", self.header_node.right.get().raw_ptr());
                    println!("rem end tail: {:?}", self.tail_node.left.get().raw_ptr());
                    return Some(val);
                }
            } else {
                let tail = self
                    .tail_node
                    .left
                    .get_full();
                let tail = tail
                    .get_ptr()
                    .cast::<RawNode<T, { NodeKind::Unbound }>>();
                let tail = Aligned(unsafe { Arc::from_raw(tail) });
                // increase the ref count because we want to return a reference to the node and thus we have to create a reference out of thin air
                mem::forget(tail.clone());
                if let Some(val) = tail.remove() {
                    println!("removed tail!");
                    println!("rem end head: {:?}", self.header_node.right.get().raw_ptr());
                    println!("rem end tail: {:?}", self.tail_node.left.get().raw_ptr());
                    return Some(val);
                }
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.header_node.right.get().get_ptr() == self.tail_addr()
    }

    // FIXME: add len getter!
    /*pub fn len(&self) -> usize {
        let mut prev = self.header_node.load(Self::ENDS_ORDERING);
        let mut len = 0;
        loop {
            if prev.is_null() {
                return len;
            }
            len += 1;
            loop {
                // FIXME: how can we be sure that the node we are visiting isn't getting removed currently (this can probably be achieved by checking the REMOVE_MARKER)
                // FIXME: but how can we ensure that we are behaving correctly - and what does correct behavior in this context even mean?
            }
        }
    }*/

    // FIXME: add iter method!
}

impl<T, const NODE_KIND: NodeKind> Drop for AtomicDoublyLinkedList<T, NODE_KIND> {
    fn drop(&mut self) {
        // remove all nodes in the list, when the list gets dropped,
        // this makes sure that all the nodes' state is consistent
        // and correct
        while !self.is_empty() {
            self.remove_head();
        }
        unsafe {
            self.header_node.right.set_unsafe(null());
            self.tail_node.left.set_unsafe(null());
        }
    }
}

#[derive(Copy, Clone, Default, Debug, Eq, PartialEq)]
pub enum NodeKind {
    /// nodes of this kind will remain inside the list, even if the
    /// reference returned by the add function gets dropped
    #[default]
    Bound,
    /// nodes of this kind will immediately be removed from the list
    /// once the reference returned by the add function gets dropped
    Unbound,
}

// we have to ensure, that the first 2 bits of this tye's pointer aren't used,
// in order to accomplish this we are always using this type in combination with Aligned<4, Self>
// #[derive(Debug)]
pub struct AtomicDoublyLinkedListNode<
    T,
    const NODE_KIND: NodeKind = { NodeKind::Bound },
    const DELETED: bool = false,
> {
    // TODO: change the bool here into an enum!
    val: MaybeUninit<T>,
    left: Link<T, NODE_KIND>,
    right: Link<T, NODE_KIND>,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<T> AtomicDoublyLinkedListNode<T, { NodeKind::Bound }, false> {
    pub fn remove(self: &Aligned<A4, Arc<Aligned<A4, Self>>>) -> Option<Node<T, { NodeKind::Bound }, true>> {
        // let this = Arc::as_ptr(self) as *const RawNode<T, { NodeKind::Bound }>;
        // let _tmp = self.left.get();
        // println!("_tmp interm counter: {} | curr counter: {}", _tmp.ptr.parent.intermediate_ref_cnt.load(Ordering::SeqCst), _tmp.ptr.parent.curr_ref_cnt.load(Ordering::SeqCst));
        if self.right.get().raw_ptr().is_null() || self.left.get()/*_tmp*/.raw_ptr().is_null() {
            // we can't remove the header or tail nodes
            return None;
        }
        // drop(_tmp);
        let mut prev;
        loop { // FIXME: for some reason this loop never ends!
            let next = self.right.get_full()/*self.right.get()*/;
            if next.get_deletion_marker() {
                // FIXME: do we need to drop the arc here as well? - we probably don't because the deletion marker on next (probably) means that this(`self`) node is already being deleted
                /*if !next.get_ptr().is_null() &&
                    self.right.ptr.compare_exchange(next.ptr.cast_mut(), ptr::invalid_mut(DELETION_MARKER), Ordering::SeqCst, strongest_failure_ordering(Ordering::SeqCst)).is_ok() {
                    unsafe { release_ref(next.get_ptr()); }
                }*/
                return None;
            }
            /*println!("pre set right!");
            println!("right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
            let tmp = self.right.get();
            let tmp_right = ManuallyDrop::new(unsafe { Arc::from_raw(tmp.get_ptr()) });
            // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
            println!("right arc refs: {}", Arc::strong_count(&tmp_right));
            drop(tmp);
            println!("curr right: {:?}", self.right.get().raw_ptr());
            println!("next: {:?}", next.raw_ptr());*/
            if self.right.try_set_deletion_marker(next.raw_ptr()) {
                println!("did set right!");
                loop {
                    prev = self.left.get_full()/*self.left.get()*/;
                    if prev.get_deletion_marker()
                        || self.left.try_set_deletion_marker(prev.raw_ptr())
                    {
                        break;
                    }
                }
                /*println!("pre head stuff!");
                println!("right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
                let tmp = self.right.get();
                let tmp_right = ManuallyDrop::new(unsafe { Arc::from_raw(tmp.get_ptr()) });
                // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
                println!("right arc refs: {}", Arc::strong_count(&tmp_right));
                drop(tmp);
                let prev_tmp = ManuallyDrop::new(unsafe { Arc::from_raw(prev.get_ptr()) });
                println!("left pre ptr: {:?}", self.left.get().raw_ptr());
                println!("right pre ptr: {:?}", self.right.get().raw_ptr());*/
                let prev_tmp = ManuallyDrop::new(unsafe { Arc::from_raw(prev.get_ptr()) });
                if let PtrGuardOrPtr::FullGuard(guard) = prev_tmp
                    .correct_prev::<true>(/*leak_arc(unsafe { Arc::from_raw(next.get_ptr()) })*/next.get_ptr()) { // FIXME: PROBABLY: next is already freed when it gets derefed again
                    prev = FullLinkContent {
                        ptr: guard, // FIXME: we get an unwrapped none panic in this line (in Bound mode) - that's probably because here we have a header node which we try to deref!
                    };
                }
                /*println!("post head stuff!");
                println!("left post ptr: {:?}", self.left.get().raw_ptr());
                println!("right post ptr: {:?}", self.right.get().raw_ptr());*/
                /*unsafe {
                    release_ref(prev.get_ptr());
                    release_ref(next.get_ptr());
                    // release_ref(self); // we probably don't need this as we return a reference from this function
                }*/

                // SAFETY: This is safe because we know that we leaked a reference to the arc earlier,
                // so we can just reduce the reference count again such that the `virtual reference`
                // we held is gone.
                let ret = unsafe { Arc::from_raw(Arc::as_ptr(self)) };
                // mem::forget(ret.clone()); // FIXME: this fixes use after free, but the list still isn't correct


                println!("ret_refs: {}", Arc::strong_count(&ret));

                // SAFETY: This is safe because the only thing we change about the type
                // is slightly tightening its usage restrictions and the in-memory
                // representation of these two types is exactly the same as the
                // only thing that changes is a const attribute
                return Some(unsafe { transmute(ret) });
            }
        }
    }
}

impl<T> AtomicDoublyLinkedListNode<T, { NodeKind::Unbound }, false> {
    pub fn remove(self: Aligned<A4, Arc<Aligned<A4, Self>>>) -> Option<Node<T, { NodeKind::Bound }, true>> {
        /*fn inner_remove<T>(slf: &Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, { NodeKind::Unbound }, false>>>) -> Option<()> {
            // let this = Arc::as_ptr(self) as *const RawNode<T, { NodeKind::Unbound }>;
            if slf.right.get().raw_ptr().is_null() || slf.left.get().raw_ptr().is_null() {
                // we can't remove the header or tail nodes
                return None;
            }
            let mut prev;
            loop {
                let next = slf.right.get();
                if next.get_deletion_marker() {
                    // FIXME: do we need to drop the arc here as well? - we probably don't because the deletion marker on next (probably) means that this(`self`) node is already being deleted
                    return None;
                }
                println!("pre set right!");
                if slf.right.try_set_deletion_marker(next.raw_ptr()) {
                    println!("did set right!");
                    loop {
                        prev = slf.left.get();
                        if prev.get_deletion_marker()
                            || slf.left.try_set_deletion_marker(prev.raw_ptr())
                        {
                            break;
                        }
                    }
                    println!("pre head stuff!");
                    let prev_tmp = ManuallyDrop::new(unsafe { Arc::from_raw(prev.get_ptr()) });
                    // let next_tmp = ManuallyDrop::new(unsafe { Arc::from_raw(next.get_ptr()) });
                    if let PtrGuardOrPtr::Guard(guard) = prev_tmp
                        .correct_prev::<true>(leak_arc(unsafe { Arc::from_raw(next.get_ptr()) })/*&next_tmp*/) {
                        prev = LinkContent {
                            ptr: guard, // FIXME: we get an unwrapped none panic in this line (in Bound mode) - that's probably because here we have a header node which we try to deref!
                        };
                    }
                    println!("post head stuff!");
                    println!("left del: {}", slf.left.get().get_deletion_marker());
                    println!("right del: {}", slf.left.get().get_deletion_marker());

                    return Some(());
                }
            }
        }
        inner_remove::<T>(&self).map(|slf| {
            // SAFETY: This is safe because the only thing we change about the type
            // is slightly tightening its usage restrictions and the in-memory
            // representation of these two types is exactly the same as the
            // only thing that changes is a const attribute
            return unsafe { transmute(self) };
        })*/
        todo!()
    }
}

impl<T, const NODE_KIND: NodeKind> AtomicDoublyLinkedListNode<T, NODE_KIND, false> {
    pub fn add_after(self: &Arc<Aligned<A4, Self>>, val: T) -> Node<T, NODE_KIND> {
        let node = Aligned(Arc::new/*pin*/(Aligned(AtomicDoublyLinkedListNode {
            val: MaybeUninit::new(val),
            left: Link::invalid(),
            right: Link::invalid(),
        })));

        if NODE_KIND == NodeKind::Bound {
            let _ = ManuallyDrop::new(node.clone()); // leak a single reference for when we are removing the node
        }

        self.inner_add_after(&node);
        node
    }

    // ptr->ptr->Node
    fn inner_add_after(self: &Arc<Aligned<A4, Self>>, node: &Arc<RawNode<T, NODE_KIND>>) {
        /*println!("cnt: {}", COUNTER.fetch_add(1, Ordering::SeqCst));
        println!("PRE right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
        let tmp = self.right.get();
        println!("right: {:?}", tmp.raw_ptr());
        let tmp_right = ManuallyDrop::new(unsafe { Arc::from_raw(tmp.get_ptr()) });
        // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
        println!("PRE right arc refs: {}", Arc::strong_count(&tmp_right));*/
        let this = Arc::as_ptr(self);
        if self.right./*get_full()*/get().raw_ptr().is_null() {
            println!("adding beffffore!");
            // if we are the tail, add before ourselves, not after
            return self.inner_add_before(node);
        }
        // println!("after check!");
        /*println!("POST right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
        // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
        println!("POST right arc refs: {}", Arc::strong_count(&tmp_right));
        drop(tmp);*/
        // thread::sleep(Duration::from_millis(100));
        let mut back_off_weight = 1;
        // let node_ptr = Arc::as_ptr(node);
        let mut next;
        loop {
            println!("in loop: {} | {}", self.right.ptr.intermediate_ref_cnt.load(Ordering::Acquire), self.right.ptr.curr_ref_cnt.load(Ordering::Acquire));
            next = self.right.get_full()/*self.right.get()*/; // = tail // FIXME: MIRI flags this because apparently the SwapArc here gets dropped while performing the `get_full` operation.
            println!("got full!");
            unsafe {
                // create_ref(this); // store_ref(node.left, <prev, false>) | in iter 1: head refs += 1

                // increase the ref count by 1
                // mem::forget(self.clone());
                // create_ref(this);
                node.left.set_unsafe/*::<false>*/(this);
            }
            unsafe {
                // create_ref(next.get_ptr()); // store_ref(node.right, <next, false>) | in iter 1: tail refs += 1

                // increase the ref count by 1
                // create_ref(next.get_ptr());
                node.right.set_unsafe/*::<false>*/(next.get_ptr()); // FIXME: for some reason the value which was here before may already have been freed, so this can lead to a use after free!
            }

            println!("set meta!");
            if self
                .right
                .try_set_addr_full_with_meta(next.get_ptr(), Arc::as_ptr(node))
                // .try_set_addr_full/*::<false>*/(next.get_ptr(), node.clone())
            {
                // SAFETY: we have to create a reference to node in order ensure, that it is always valid, when it has to be
                // unsafe { create_ref(Arc::as_ptr(node)); }
                break;
            }
            // unsafe { release_ref(next.get_ptr()); } // in iter 1: tail refs -= 1
            if self.right.get_meta().get_deletion_marker() { // FIXME: err when using get() instead of get_meta()
                // release_ref(node); // this is probably unnecessary, as we don't delete the node, but reuse it
                // delete_node(node);
                println!("adding beffffore!");
                return self.inner_add_before(node);
            }
            // back-off
            thread::sleep(Duration::from_micros(10 * back_off_weight));
            back_off_weight += 1;
        }
        println!("added after!");
        // *cursor = node;
        let _prev/*prev*/ = self.correct_prev::<false>(next.get_ptr()); // FIXME: this isn't even called until it panics, so this can't be the cause!
        // unsafe { release_ref_2(prev, next.get_ptr()); }
        /*drop(next);
        println!("right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
        let tmp = self.right.get();
        let tmp_right = ManuallyDrop::new(unsafe { Arc::from_raw(tmp.get_ptr()) });
        // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
        println!("right arc refs: {}", Arc::strong_count(&tmp_right));*/
        /*println!("right arc refs: {} | left arc refs: {}", Arc::strong_count(leak_arc(unsafe { Arc::from_raw(self.right.get().get_ptr().as_ref().unwrap()) })),
                 Arc::strong_count(leak_arc(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) })));*/
    }

    pub fn add_before(self: &Arc<Aligned<A4, Self>>, val: T) -> Node<T, NODE_KIND> {
        let node = Aligned(Arc::new/*pin*/(Aligned(AtomicDoublyLinkedListNode {
            val: MaybeUninit::new(val),
            left: Link::invalid(),
            right: Link::invalid(),
        })));

        if NODE_KIND == NodeKind::Bound {
            mem::forget(node.clone()); // leak a single reference
        }

        self.inner_add_before(&node);
        node
    }

    // FIXME: the original code uses `pointer to pointer to Node` instead of `pointer to Node` as we do, is this the main bug? - it probably is
    // ptr->ptr->Node
    fn inner_add_before(self: &Arc<Aligned<A4, Self>>, node: &Arc<RawNode<T, NODE_KIND>>) {
        if self.left.get().raw_ptr().is_null() {
            // if we are the header, add after ourselves, not before
            return self.inner_add_after(node);
        }
        let mut back_off_weight = 1;
        let this = Arc::as_ptr(self);
        // let node_ptr = Arc::as_ptr(node);
        let mut i = 0;
        let left = self.left.get_full();
        let mut prev = PtrGuardOrPtr::FullGuard(left.ptr);
        let /*mut */cursor = this;
        // let mut next = cursor;
        loop {
            i += 1;
            println!("in add before loop: {}", i);
            while self.right.get_meta().get_deletion_marker() {
                /*if let Some(node) = unsafe { cursor.as_ref() }.unwrap().clone().next() {
                    cursor = node;
                }*/
                unsafe { cursor.as_ref() }.unwrap().clone().next();
                let prev_tmp = unsafe { Arc::from_raw(prev.as_ptr_no_meta()) };
                prev = prev_tmp
                    .correct_prev::<false>(this/*cursor.cast_mut()*/);
                mem::forget(prev_tmp);
                println!("found del marker!");
            }
            // next = cursor;
            unsafe {
                // create_ref(prev); // store_ref(node.left, <prev, false>)
                // create_ref(prev.as_ptr());
                node.left.set_unsafe/*::<false>*/(prev.as_ptr_no_meta());
            }
            unsafe {
                // create_ref(next); // store_ref(node.right, <next, false>)
                // create_ref(cursor);
                node.right.set_unsafe/*::<false>*/(cursor.cast_mut());
            }
            if unsafe { prev.as_ptr_no_meta().as_ref().unwrap() }
                .right
                .try_set_addr_full_with_meta(cursor.cast_mut(), Arc::as_ptr(node))
                /*.try_set_addr_full/*::<false>*/(cursor.cast_mut(), node.clone())*/
            {
                break;
            }
            let prev_tmp = unsafe { Arc::from_raw(prev.as_ptr_no_meta()) };
            prev = prev_tmp
                .correct_prev::<false>(this/*cursor.cast_mut()*/);
            mem::forget(prev_tmp);
            // back-off
            thread::sleep(Duration::from_micros(10 * back_off_weight));
            back_off_weight += 1;
        }
        // *cursor = node;
        let prev_tmp = unsafe { Arc::from_raw(prev.as_ptr_no_meta()) };
        let _drop = prev_tmp
            .correct_prev::<false>(this/*next.cast_mut()*/);
        mem::forget(prev_tmp);
        // unsafe { release_ref_2(prev, next); }
    }

    /// Tries to set the current node's right link
    /// to its following node.
    /// In pseudocode: self.right = self.right.right;
    /// Note, that there are additional deletion checks
    /// being performed before setting the next node.
    /// This method returns true as long as the tail node isn't reached.
    // pointer to pointer to node
    fn next(self: &Aligned<A4, Self>) -> Option<*const RawNode<T, NODE_KIND>> {
        // prepped for header/tail update
        let mut cursor = self as *const RawNode<T, NODE_KIND>;
        loop {
            let next = unsafe { cursor.as_ref() }.unwrap().right.get();
            if next.raw_ptr().is_null() {
                // check if the cursor is the tail, if so - return false
                return None;
            }
            let marker = unsafe { next.get_ptr().as_ref() }
                .unwrap()
                .right
                .get()
                .get_deletion_marker();
            if marker
                && unsafe { cursor.as_ref() }.unwrap().right.get().raw_ptr()
                    != next.raw_ptr().map_addr(|x| x | DELETION_MARKER)
            {
                unsafe { next.get_ptr().as_ref() }
                    .unwrap()
                    .left
                    .set_deletion_mark();
                let new_right = unsafe { next.get_ptr().as_ref() }.unwrap().right.get();
                unsafe { cursor.as_ref() }
                    .unwrap()
                    .right
                    .try_set_addr_full_with_meta/*::<false>*/(next.raw_ptr(), new_right.get_ptr());
                // unsafe { release_ref(next.get_ptr()); }
                continue;
            }
            // unsafe { release_ref(cursor); }
            cursor = next.get_ptr();
            if !marker
                && !unsafe { next.get_ptr().as_ref() }
                    .unwrap()
                    .right
                    .get()
                .raw_ptr()
                    .is_null()
            {
                return Some(cursor);
            }
        }
    }

    // FIXME: add prev()

    // header -> node -> this
    // header <- node | header <- this
    // node.correct_prev(this)

    // header -> node | this -> node
    // header <- this <- node


    // head.correct_prev(next) where next = tail, so: head.correct_prev(tail)
    /// tries to update the prev pointer of a node and then return a reference to a possibly
    /// logically previous node
    // ? - type is not annotated
    fn correct_prev<'a, const FROM_DELETION: bool>( // FIXME: this method probably doesn't work correctly! - it probably leads to `self` being dropped for some reason in some cases
        self: &Arc<Aligned<A4, Self>>,              // FIXME: notable NO SwapArc isn't dropped itself tho
        node: NodePtr<T, NODE_KIND>/*&Arc<RawNode<T, NODE_KIND>>*//*NodePtr<T, NODE_KIND>*/,
    ) -> PtrGuardOrPtr<'a, AtomicDoublyLinkedListNode<T, NODE_KIND>> {
        println!("start correct prev!");
        let node = ManuallyDrop::new(unsafe { Arc::from_raw(node) });
        // let id = rand::random::<usize>();
        // FIXME: currently there is an issue where we go further and further to the right without finding from `self` without finding the `node` we are looking for
        // let initial = Arc::as_ptr(self); // = node
        let mut back_off_weight = 1;
        let mut prev = PtrGuardOrPtr::Ptr(Arc::as_ptr(self)); // = node
        let mut last_link: Option<PtrGuardOrPtr<'_, AtomicDoublyLinkedListNode<T, NODE_KIND>>/*NodePtr<T, NODE_KIND>*/> = None;
        let mut i = 0;
        loop {
            /*if i >= 10 {
                loop {}
            }*/
            i += 1;
            // println!("in correct prev loop {} del: {}", i, FROM_DELETION);
            let link_1 = node.left.get_full(); // FIXME: get this without guard, as we don't need one
            if link_1.get_deletion_marker() {
                break;
            }
            let link_1 = link_1.raw_ptr();
            // println!("prev: {:?} | init: {:?}", prev, initial);
            let mut prev_2 = unsafe { prev.as_ptr_no_meta().as_ref() }.unwrap().right.get_full(); // = this | FIXME: this sometimes has a dangling pointer (as reported by MIRI)
            // println!("in correct prev loop {} del: {}\nprev: {:?} | init: {:?} | node: {:?} | prev_2: {:?} | id: {id}", i, FROM_DELETION, prev, initial, node, prev_2.get_ptr());
            if prev_2.get_deletion_marker() {
                println!("del marker!");
                // loop {}
                if let Some(last_link) = last_link.take() {
                    unsafe { prev.as_ptr_no_meta().as_ref() }.unwrap().left.set_deletion_mark();
                    unsafe { last_link.as_ptr_no_meta().as_ref().unwrap() }
                        .right
                        .try_set_addr_full_with_meta/*::<false>*/(prev.as_ptr(), prev_2.get_ptr());
                    // unsafe { release_ref_2(prev_2.get_ptr(), prev); }
                    prev = last_link;
                    continue;
                }
                // unsafe { release_ref(prev_2.get_ptr()); }
                // let old_prev_2 = mem::replace(&mut prev_2, unsafe { prev.as_ptr().as_ref() }.unwrap().left.get());
                prev_2 = unsafe { prev.as_ptr_no_meta().as_ref() }.unwrap().left.get_full();
                // unsafe { release_ref(prev); }
                prev = PtrGuardOrPtr::FullGuard(prev_2.ptr.clone()/*old_prev_2.ptr*/);
                println!("continuing after del marker check {:?}", prev_2.raw_ptr());
                continue;
            }
            if /*prev_2.get_ptr()*/prev_2.raw_ptr() != Arc::as_ptr(&*node) {
                println!("prev_2: {:?}", prev_2.raw_ptr());
                // println!("node: {:?}", node);
                if let Some(last_link) = last_link.replace(prev) {
                    // unsafe { release_ref(last_link); }
                }
                println!("after exchange!");
                prev = PtrGuardOrPtr::FullGuard(prev_2.ptr.clone());
                println!("continuing after prev_2 = node check");
                continue;
            }
            // unsafe { release_ref(prev_2.get_ptr()); } // tail refs -= 1

            if node
                .left
                .try_set_addr_full_with_meta/*::<false>*/(link_1, prev.as_ptr_no_meta())
            {
                println!("try finishing UP!");
                if unsafe { prev.as_ptr_no_meta().as_ref() }
                    .unwrap()
                    .left
                    .get_meta()
                    .get_deletion_marker()
                {
                    println!("continuing in the end!");
                    continue;
                }
                break;
            }
            // back-off
            thread::sleep(Duration::from_micros(10 * back_off_weight));
            back_off_weight += 1;
        }
        println!("finished correct prev!");
        /*if let Some(last_link) = last_link.take() {
            unsafe { release_ref(last_link); } // head refs -= 1
        }*/
        if prev.as_ptr().is_null() {
            println!("prev is null!");
        } else {
            let tmp = ManuallyDrop::new(unsafe { Arc::from_raw(prev.as_ptr_no_meta()) });
            println!("prev refs: {}", Arc::strong_count(&tmp));
        }
        prev
    }
}

impl<T, const NODE_KIND: NodeKind, const DELETED: bool>
    AtomicDoublyLinkedListNode<T, NODE_KIND, DELETED>
{
    /// checks whether the node is detached from the list or not
    /*pub fn is_detached(&self) -> bool {
        // we can assume that the node is detached if DELETED is true,
        // but in all cases other than `DELETED = true` we have to check for left's validity to ensure that we can return false
        DELETED || self.left.is_invalid()
    }*/

    #[inline]
    pub fn get(&self) -> Option<&T> {
        let right = self.right.get();
        if right.get_deletion_marker() || right.raw_ptr().is_null() || self.left.get().raw_ptr().is_null() {
            return None;
        }
        // SAFETY: we have just checked that we are not the header or tail nodes and thus our
        // value has to be init!
        Some(unsafe { self.val.assume_init_ref() })
    }

    /*
    // FIXME: should we maybe consume Arc<Self> here?
    pub fn next(&self) -> Option<Arc<AtomicBoxedDoublyLinkedListNode<T>>> {
        unsafe { self.right.load(Ordering::Acquire).as_ref() }.map(|x| x.clone())
    }

    // FIXME: should we maybe consume Arc<Self> here?
    pub fn prev(&self) -> Option<Arc<AtomicBoxedDoublyLinkedListNode<T>>> {
        unsafe { self.left.load(Ordering::Acquire).as_ref() }.map(|x| x.clone())
    }*/
}

impl<T, const NODE_KIND: NodeKind, const DELETED: bool> Drop
    for AtomicDoublyLinkedListNode<T, NODE_KIND, DELETED>
{
    fn drop(&mut self) {
        if self.right.get().raw_ptr().is_null() || self.left.get().raw_ptr().is_null() {
            // don't do anything when the header or tail nodes get dropped
            return;
        }
        /*
        if NODE_KIND == NodeKind::Unbound && !DELETED { // FIXME: add an detached marker and check it here!
            // FIXME: remove this node from the list! - put this code inside a wrapper
        }*/
        /*unsafe {
            release_ref(self.right.get().get_ptr());
            release_ref(self.left.get().get_ptr());
        }*/
        // SAFETY: this is safe because this is the only time
        // the node and thus `val` can be dropped
        unsafe {
            self.val.assume_init_drop();
        }
    }
}

const DELETION_MARKER: usize = 1 << 1/*63*/;
const DETACHED_MARKER: usize = 1 << 0/*62*/; // FIXME: can we replace this marker with nulling pointers?

// #[derive(Debug)]
struct Link<T, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    // ptr: Aligned<A4, AtomicPtr<RawNode<T, NODE_KIND>>>,
    ptr: Aligned<A4, Arc<SwapArcIntermediateTLS<RawNode<T, NODE_KIND>, Option<Aligned<A4, Arc<RawNode<T, NODE_KIND>>>>/*Option<Arc<RawNode<T, NODE_KIND>>>*/, 2>>>,
}

impl<T, const NODE_KIND: NodeKind> Link<T, NODE_KIND> {
    const CAS_ORDERING: Ordering = Ordering::SeqCst;

    fn get(&self) -> LinkContent<'_, T, NODE_KIND/*SwapArcIntermediatePtrGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>*//*, LinkContent<T, NODE_KIND>*/>/*LinkContent<T, NODE_KIND>*//*SwapArcIntermediateGuard<'_, LinkContent<T, NODE_KIND>>*/ {
        LinkContent {
            ptr: unsafe { self.ptr.load_raw() },
        }
    }

    fn get_full(&self) -> FullLinkContent<T, NODE_KIND> {
        FullLinkContent {
            ptr: unsafe { self.ptr.load_raw_full() },
        }
    }

    /*
    fn get_typed(&self) -> SwapArcIntermediateGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2> {
        self.ptr.load()
    }*/

    fn get_meta(&self) -> Metadata {
        Metadata(self.ptr.load_metadata())
    }

    fn set_deletion_mark(&self) {
        /*let mut node = self.get();
        'retry: loop {
            if node.get_deletion_marker() {
                break;
            }
            let cmp = self
                .ptr
                .compare_exchange_weak(
                    node.ptr.cast_mut(),
                    ptr::from_exposed_addr_mut(node.ptr.expose_addr() | DELETION_MARKER),
                    Self::CAS_ORDERING,
                    strongest_failure_ordering(Self::CAS_ORDERING),
                );
            match cmp {
                Ok(_) => {
                    break 'retry;
                }
                Err(curr_val) => {
                    // retry if exchange failed
                    node = LinkContent {
                        ptr: curr_val.cast_const(),
                    };
                }
            }
        }*/
        self.ptr.set_in_metadata(DELETION_MARKER);
    }

    /*
    fn try_set_addr_full/*<const SET_DELETION_MARKER: bool>*/(
        &self,
        old: NodePtr<T, NODE_KIND>,
        new: Aligned<A4, Arc<RawNode<T, NODE_KIND>>>/*&SwapArcIntermediateGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>,*/
    ) -> bool {
        /*unsafe { create_ref(new); } // FIXME: don't create references prematurely before even knowing whether the update will succeed or not

        let res = self.ptr
            .compare_exchange(
                old.cast_mut(),
                if SET_DELETION_MARKER {
                    ptr::from_exposed_addr_mut(new.expose_addr() | DELETION_MARKER)
                } else {
                    new.cast_mut()
                },
                Self::CAS_ORDERING,
                strongest_failure_ordering(Self::CAS_ORDERING),
            )
            .is_ok();

        if res {
            unsafe { release_ref(old); } // decrease the old reference count
        } else {
            unsafe { release_ref(new); } // decrease the new reference count if the update failed
        }

        res*/
        unsafe { self.ptr.try_compare_exchange::<false>(old, Some(new)) }
    }*/

    fn try_set_addr_full_with_meta/*<const SET_DELETION_MARKER: bool>*/(
        &self,
        old: NodePtr<T, NODE_KIND>,
        new: NodePtr<T, NODE_KIND>/*&SwapArcIntermediateGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>,*/
    ) -> bool {
        /*unsafe { create_ref(new); } // FIXME: don't create references prematurely before even knowing whether the update will succeed or not

        let res = self.ptr
            .compare_exchange(
                old.cast_mut(),
                if SET_DELETION_MARKER {
                    ptr::from_exposed_addr_mut(new.expose_addr() | DELETION_MARKER)
                } else {
                    new.cast_mut()
                },
                Self::CAS_ORDERING,
                strongest_failure_ordering(Self::CAS_ORDERING),
            )
            .is_ok();

        if res {
            unsafe { release_ref(old); } // decrease the old reference count
        } else {
            unsafe { release_ref(new); } // decrease the new reference count if the update failed
        }

        res*/
        unsafe { self.ptr.try_compare_exchange_with_meta(old, new) }
    }

    fn try_set_deletion_marker/*<const SET_DELETION_MARKER: bool>*/(&self, old: NodePtr<T, NODE_KIND>) -> bool {
        // self.try_set_addr_full::<true/*SET_DELETION_MARKER*/>(old, old)
        self.ptr.try_update_meta(old, DELETION_MARKER)
    }

    // SAFETY: This may only be used on new nodes that are in no way
    // linked to and that link to nowhere.
    unsafe fn set_unsafe/*<const SET_DELETION_MARKER: bool>*/(&self, new: NodePtr<T, NODE_KIND>) {
        /*let marker = if SET_DELETION_MARKER {
            DELETION_MARKER
        } else {
            0
        };*/
        /*create_ref(new);
        self.ptr.store(
            new.cast_mut()/*ptr::from_exposed_addr_mut(new.expose_addr() | marker)*/,
            /*Ordering::Relaxed*/Self::CAS_ORDERING,
        );
        */
        self.ptr.update_raw(new);



        /*
        let prev = self.ptr.swap(
            new.cast_mut()/*ptr::from_exposed_addr_mut(new.expose_addr() | marker)*/,
            /*Ordering::Relaxed*/Self::CAS_ORDERING,
        );
        let prev = LinkContent {
            ptr: prev,
        };
        if !prev.get_ptr().is_null() {

        }
        */
    }

    fn cleanup(&self) {
        // FIXME: once the DETACHED flag is properly supported everywhere, set the ptr to DETACHED (and MAYBE add the DELETION flag)
    }

    /*
    unsafe fn invalidate(&self) {
        self.ptr.store(null_mut(), Self::CAS_ORDERING);
    }

    fn is_invalid(&self) -> bool {
        self.ptr.load(Self::CAS_ORDERING).is_null()
    }*/

    #[inline]
    fn invalid() -> Self {
        Self {
            ptr: Aligned(SwapArcIntermediateTLS::new(None)),
        }
    }
}

#[inline]
#[cfg(target_has_atomic = "8")]
fn strongest_failure_ordering(order: Ordering) -> Ordering {
    match order {
        Ordering::Release => Ordering::Relaxed,
        Ordering::Relaxed => Ordering::Relaxed,
        Ordering::SeqCst => Ordering::SeqCst,
        Ordering::Acquire => Ordering::Acquire,
        Ordering::AcqRel => Ordering::Acquire,
        _ => unreachable!(),
    }
}

// FIXME: always pass this around wrapped inside a guard!
// #[derive(Copy, Clone, Debug)]
struct LinkContent<'a, T, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    // ptr: NodePtr<T, NODE_KIND>,
    ptr: SwapArcIntermediatePtrGuard<'a, RawNode<T, NODE_KIND>, Option<Aligned<A4, Arc<RawNode<T, NODE_KIND>>>>, 2>,
}

impl<T, const NODE_KIND: NodeKind> LinkContent<'_, T, NODE_KIND> {
    fn get_deletion_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DELETION_MARKER) != 0
    }

    fn get_detached_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DETACHED_MARKER) != 0
    }

    fn get_ptr(&self) -> NodePtr<T, NODE_KIND> {
        self.raw_ptr().map_addr(|x| x & !(DELETION_MARKER | DETACHED_MARKER))
    }

    /*
    fn get_val(&self) -> Option<Arc<RawNode<T, NODE_KIND>>> {
        if !self.get_ptr().is_null() {
            Some(unsafe { Arc::from_raw(self.get_ptr()) })
        } else {
            None
        }
    }*/

    #[inline]
    fn raw_ptr(&self) -> NodePtr<T, NODE_KIND> {
        self.ptr.as_raw()
    }
}

/*
impl<T, const NODE_KIND: NodeKind> Clone for LinkContent<T, NODE_KIND> {
    fn clone(&self) -> Self {
        if !self.get_ptr().is_null() {
            let tmp = unsafe { Arc::from_raw(self.get_ptr()) };
            mem::forget(tmp.clone());
            mem::forget(tmp);
        }
        LinkContent {
            ptr: self.ptr,
        }
    }
}*/

// unsafe impl<T, const NODE_KIND: NodeKind> RefCnt for LinkContent<T, NODE_KIND> {}

/*
impl<T, const NODE_KIND: NodeKind> DataPtrConvert<RawNode<T, NODE_KIND>> for LinkContent<T, NODE_KIND> {
    const INVALID: *const RawNode<T, NODE_KIND> = null();

    fn from(ptr: *const RawNode<T, NODE_KIND>) -> Self {
        Self {
            ptr,
        }
    }

    fn into(self) -> *const RawNode<T, NODE_KIND> {
        self.ptr
    }
}*/

/*
impl<'a, T, const NODE_KIND: NodeKind> Drop for LinkContent<'a, T, NODE_KIND> {
    fn drop(&mut self) {
        if !self.get_ptr().is_null() {
            unsafe { Arc::from_raw(self.get_ptr()) };
        }
    }
}*/

impl<T, const NODE_KIND: NodeKind> PartialEq for LinkContent<'_, T, NODE_KIND> {
    fn eq(&self, other: &Self) -> bool {
        self.raw_ptr().expose_addr() == other.raw_ptr().expose_addr()
    }
}

// FIXME: always pass this around wrapped inside a guard!
// #[derive(Copy, Clone, Debug)]
struct FullLinkContent<T, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    // ptr: NodePtr<T, NODE_KIND>,
    ptr: SwapArcIntermediateFullPtrGuard<RawNode<T, NODE_KIND>, Option<Aligned<A4, Arc<RawNode<T, NODE_KIND>>>>, 2>,
}

impl<T, const NODE_KIND: NodeKind> FullLinkContent<T, NODE_KIND> {
    fn get_deletion_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DELETION_MARKER) != 0
    }

    fn get_detached_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DETACHED_MARKER) != 0
    }

    fn get_ptr(&self) -> NodePtr<T, NODE_KIND> {
        self.raw_ptr().map_addr(|x| x & !(DELETION_MARKER | DETACHED_MARKER))
    }

    /*
    fn get_val(&self) -> Option<Arc<RawNode<T, NODE_KIND>>> {
        if !self.get_ptr().is_null() {
            Some(unsafe { Arc::from_raw(self.get_ptr()) })
        } else {
            None
        }
    }*/

    #[inline]
    fn raw_ptr(&self) -> NodePtr<T, NODE_KIND> {
        self.ptr.as_raw()
    }
}

#[derive(Copy, Clone)]
struct Metadata(usize);

impl Metadata {

    fn get_deletion_marker(&self) -> bool {
        (self.0 & DELETION_MARKER) != 0
    }

    fn get_detached_marker(&self) -> bool {
        (self.0 & DETACHED_MARKER) != 0
    }

}

enum PtrGuardOrPtr<'a, T> {
    Guard(SwapArcIntermediatePtrGuard<'a, Aligned<A4, T>, Option<Aligned<A4, Arc<Aligned<A4, T>>>>, 2>),
    FullGuard(SwapArcIntermediateFullPtrGuard<Aligned<A4, T>, Option<Aligned<A4, Arc<Aligned<A4, T>>>>, 2>),
    Ptr(*const Aligned<A4, T>),
}

impl<T> PtrGuardOrPtr<'_, T> {

    const META_MASK: usize = ((1 << 0) | (1 << 1));

    fn as_ptr(&self) -> *const Aligned<A4, T> {
        match self {
            PtrGuardOrPtr::Guard(guard) => guard.as_raw(),
            PtrGuardOrPtr::Ptr(ptr) => *ptr,
            PtrGuardOrPtr::FullGuard(full_guard) => full_guard.as_raw(),
        }
    }

    fn as_ptr_no_meta(&self) -> *const Aligned<A4, T> {
        self.as_ptr().map_addr(|x| x & !Self::META_MASK)
    }

}

unsafe fn release_ref<T, const NODE_KIND: NodeKind>(node_ptr: NodePtr<T, NODE_KIND>) {
    // decrement the reference count of the arc
    Arc::from_raw(node_ptr);
}

unsafe fn release_ref_2<T, const NODE_KIND: NodeKind>(node_ptr: NodePtr<T, NODE_KIND>, node_ptr_2: NodePtr<T, NODE_KIND>) {
    // FIXME: is this the correct handling of release_ref with 2 params?
    // release_ref(node_ptr);
    // release_ref(node_ptr_2);
}

unsafe fn create_ref<T, const NODE_KIND: NodeKind>(node_ptr: NodePtr<T, NODE_KIND>) {
    let owned = Arc::from_raw(node_ptr);
    mem::forget(owned.clone());
    mem::forget(owned);
}

/*
fn leak_arc<'a, T: 'a>(val: Arc<T>) -> &'a Arc<T> {
    let ptr = addr_of!(val);
    mem::forget(val);
    unsafe { ptr.as_ref() }.unwrap()
}*/

unsafe impl<T> RefCnt for Option<Aligned<A4, Arc<Aligned<A4, T>>>> {}

impl<T> DataPtrConvert<Aligned<A4, T>> for Option<Aligned<A4, Arc<Aligned<A4, T>>>> {
    const INVALID: *const Aligned<A4, T> = null();

    fn from(ptr: *const Aligned<A4, T>) -> Self {
        if !ptr.is_null() {
            Some(Aligned(unsafe { Arc::from_raw(ptr) }))
        } else {
            None
        }
    }

    fn into(self) -> *const Aligned<A4, T> {
        match self {
            None => null(),
            Some(val) => Arc::as_ptr(&val),
        }
    }

    fn as_ptr(&self) -> *const Aligned<A4, T> {
        match self {
            None => null(),
            Some(val) => Arc::as_ptr(val),
        }
    }

    fn increase_ref_cnt(&self) {
        mem::forget(self.clone());
    }
}

type NodePtr<T, const NODE_KIND: NodeKind, const REMOVED: bool = false> =
    *const RawNode<T, NODE_KIND, REMOVED>;
type RawNode<T, const NODE_KIND: NodeKind, const REMOVED: bool = false> =
    Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, REMOVED>>;
pub type Node<T, const NODE_KIND: NodeKind, const REMOVED: bool = false> =
    /*Pin<*/Aligned<A4, Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, REMOVED>>>>/*>*/;

struct SizedBox<T> {
    // FIXME: NOTE: this thing could basically be replaced by Box::leak
    alloc_ptr: NonNull<T>,
}

impl<T> SizedBox<T> {
    const LAYOUT: Layout = Layout::from_size_align(size_of::<T>(), align_of::<T>())
        .ok()
        .unwrap(); // FIXME: can we somehow retain the error message?

    fn new(val: T) -> Self {
        // SAFETY: The layout we provided was checked at compiletime, so it has to be initialized correctly
        let alloc = unsafe { alloc(Self::LAYOUT) }.cast::<T>();
        // FIXME: add safety comment
        unsafe {
            alloc.write(val);
        }
        Self {
            alloc_ptr: NonNull::new(alloc).unwrap(), // FIXME: can we make this unchecked?
        }
    }

    fn as_ref(&self) -> &T {
        // SAFETY: This is safe because we know that alloc_ptr can't be zero
        // and because we know that alloc_ptr has to point to a valid
        // instance of T in memory
        unsafe { self.alloc_ptr.as_ptr().as_ref().unwrap_unchecked() }
    }

    fn as_mut(&mut self) -> &mut T {
        // SAFETY: This is safe because we know that alloc_ptr can't be zero
        // and because we know that alloc_ptr has to point to a valid
        // instance of T in memory
        unsafe { self.alloc_ptr.as_ptr().as_mut().unwrap_unchecked() }
    }

    fn into_ptr(self) -> NonNull<T> {
        let ret = self.alloc_ptr;
        mem::forget(self);
        ret
    }
}

impl<T> Drop for SizedBox<T> {
    fn drop(&mut self) {
        // SAFETY: This is safe to call because SizedBox can only be dropped once
        unsafe {
            ptr::drop_in_place(self.alloc_ptr.as_ptr());
        }
        // FIXME: add safety comment
        unsafe {
            dealloc(self.alloc_ptr.as_ptr().cast::<u8>(), SizedBox::<T>::LAYOUT);
        }
    }
}

/*
pub struct DropOwned {

}

pub enum DropStrategy {
    MemCpy, // this uses MaybeUninit and mem::replace
    Deref,  // this uses Option::take
}*/

pub trait OwnedDrop: Sized {

    fn drop_owned(self);

}

#[repr(transparent)]
pub struct DropOwnedMemCpy<T: OwnedDrop> {
    inner: MaybeUninit<T>,
}

impl<T: OwnedDrop> DropOwnedMemCpy<T> {
    
    pub fn new(val: T) -> Self {
        Self {
            inner: MaybeUninit::new(val),
        }
    }
    
}

impl<T: OwnedDrop> Drop for DropOwnedMemCpy<T> {
    fn drop(&mut self) {
        let owned = mem::replace(&mut self.inner, MaybeUninit::uninit());
        // SAFETY: This is safe because the previous inner value has to be
        // initialized because `DropOwnedMemCpy` can only be created with
        // an initialized value.
        unsafe { owned.assume_init() }.drop_owned();
    }
}

impl<T: OwnedDrop> Deref for DropOwnedMemCpy<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.assume_init_ref() }
    }
}

impl<T: OwnedDrop> DerefMut for DropOwnedMemCpy<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.inner.assume_init_mut() }
    }
}

impl<T: OwnedDrop> From<T> for DropOwnedMemCpy<T> {
    fn from(val: T) -> Self {
        DropOwnedMemCpy::new(val)
    }
}

/*
pub struct DropOwnedDeref<T: OwnedDrop> {
    inner: Option<T>,
}

impl<T: OwnedDrop> DropOwnedDeref<T> {

    pub fn new(val: T) -> Self {
        Self {
            inner: Some(val),
        }
    }

}

impl<T: OwnedDrop> Drop for DropOwnedDeref<T> {
    fn drop(&mut self) {
        let owned = self.inner.take();
        // SAFETY: This is safe because the previous inner value has to be
        // initialized because `DropOwnedMemCpy` can only be created with
        // an initialized value.
        unsafe { owned.unwrap_unchecked() }.drop_owned();
    }
}

impl<T: OwnedDrop> Deref for DropOwnedDeref<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.inner.as_ref().unwrap_unchecked() }
    }
}

impl<T: OwnedDrop> DerefMut for DropOwnedDeref<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.inner.as_mut().unwrap_unchecked() }
    }
}

impl<T: OwnedDrop> From<T> for DropOwnedDeref<T> {
    fn from(val: T) -> Self {
        DropOwnedDeref::new(val)
    }
}*/

#[derive(Copy, Clone, Debug)]
pub enum UpdateResult {
    Ok,
    AlreadyUpdating,
    NoUpdate,
}

const fn most_sig_set_bit(val: usize) -> Option<u32> {
    let mut i = 0;
    let mut ret = None;
    while i < usize::BITS {
        if val & (1 << i) != 0 {
            ret = Some(i);
        }
        i += 1;
    }
    ret
}

const fn assert_alignment<T, const METADATA_BITS: u32>() -> bool {
    let free_bits = most_sig_set_bit(align_of::<T>()).unwrap_or(0);
    if free_bits < METADATA_BITS + 1 {
        /*let tmp = 1 << (METADATA_BITS + 1);
        let tmp_2 = 1 << free_bits;
        panic!("The alignment of T is insufficient, expected `{}`, but found `{}`", tmp, tmp_2);*/ // FIXME: use better formatting, once available in const contexts
        panic!("The alignment of T is insufficient");
    }
    true
}

const GLOBAL_UPDATE_FLAG: usize = 1 << 0;

// FIXME: note somewhere that this data structure requires T to at least be aligned to 2 bytes
// FIXME: and that the least significant bit of T's pointer is used internally, also add a static assertion for this!
/// A `SwapArc` is a data structure that allows for an `Arc`
/// to be passed around and swapped out with other `Arc`s.
/// In order to achieve this, an internal reference count
/// scheme is used which allows for very quick, low overhead
/// reads in the common case (no update) and will sill be
/// decently fast when an update is performed, as updates
/// only consist of 3 atomic instructions. When a new
/// `Arc` is to be stored in the `SwapArc`, it first tries to
/// immediately update the current pointer (the one all readers will see)
/// (this is possible, if no other update is being performed and if there are no readers left)
/// if this fails, it will `push` the update so that it will
/// be performed by the last reader to finish reading.
/// A read consists of loading the current pointer and
/// performing a clone operation on the `Arc`, thus
/// readers are very short-lived and shouldn't block
/// updates for very long, although writer starvation
/// is possible in theory, it probably won't every be
/// observed in practice because of the short-lived
/// nature of readers.

/// This variant of `SwapArc` has wait-free reads (although
/// this is at the cost of additional atomic instructions
/// (at least 1 additional load - this will never be more than 1 load if there are no updates happening).
pub struct SwapArcIntermediateTLS<T, D: DataPtrConvert<T> = Arc<T>, const METADATA_HEADER_BITS: u32 = 0> {
    // FIXME: support metadata - how can we do that?
    // FIXME: we could maybe do this by putting the metadata inside of the curr and intermediate atomic ptrs
    // FIXME: inside the SwapArc itself - the major issue we have is that with this approach we loose basically all the benefits
    // FIXME: of using thread locals as we have to maintain the same structure inside the SwapArc as with the old `SwapArc` (without tls)
    // FIXME: this is because we have to maintain the same old ref counter using atomic fetch_add and fetch_sub instructions
    // FIXME: this prevents new updates from happening (probably)
    // FIXME: IMPORTANT: if we want to solve the metadata issue if we have to solve the compare_exchange issue first
    // FIXME: because the metadata solution has to take the compare_exchange behavior into account
    curr_ref_cnt: AtomicUsize, // the last bit is the `update` bit
    ptr: AtomicPtr<T>,
    intermediate_ref_cnt: AtomicUsize, // the last bit is the `update` bit
    intermediate_ptr: AtomicPtr<T>,
    updated: AtomicPtr<T>,
    thread_local: ThreadLocal<LocalData<T, D, METADATA_HEADER_BITS>>,
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> SwapArcIntermediateTLS<T, D, METADATA_PREFIX_BITS> {

    const UPDATE: usize = 1 << (usize::BITS - 1);
    // const FORCE_UPDATE: usize = 1 << (usize::BITS - 2); // FIXME: do we actually need a separate flag? - we probably do
    // FIXME: implement force updating!
    const OTHER_UPDATE: usize = 1 << (usize::BITS - 2);

    pub fn new(val: D) -> Arc<Self> {
        // static_assertions::const_assert!(assert_alignment::<T, { METADATA_PREFIX_BITS }>()); // FIXME: fix this!
        let val = ManuallyDrop::new(val);
        let virtual_ref = val.as_ptr();
        Arc::new(Self {
            curr_ref_cnt: Default::default(),
            ptr: AtomicPtr::new(virtual_ref.cast_mut()),
            intermediate_ref_cnt: Default::default(),
            intermediate_ptr: AtomicPtr::new(virtual_ref.cast_mut()/*null_mut()*/),
            updated: AtomicPtr::new(null_mut()),
            thread_local: ThreadLocal::new(),
        })
    }

    /// SAFETY: this is only safe to call if the caller increments the
    /// reference count of the "object" `val` points to.
    fn dummy0() {}
    /*unsafe fn new_raw(val: *const T) -> Arc<Self> {
        Arc::new(Self {
            curr_ref_cnt: Default::default(),
            ptr: AtomicPtr::new(val.cast_mut()),
            intermediate_ref_cnt: Default::default(),
            intermediate_ptr: AtomicPtr::new(null_mut()),
            updated: AtomicPtr::new(null_mut()),
            thread_local: ThreadLocal::new(),
            _phantom_data: Default::default(),
        })
    }*/

    pub fn load<'a>(self: &'a Arc<Self>) -> SwapArcIntermediateGuard<'a, T, D, METADATA_PREFIX_BITS> {
        let mut new = false;
        let parent = self.thread_local.get_or(|| {
            new = true;
            /*LocalData {
                inner: MaybeUninit::new(self.clone().load_internal()),
                ref_cnt: 1,
            }*/
            let curr = self.clone().load_internal();
            let curr_ptr = curr.fake_ref.as_ptr();
            // increase the reference count
            mem::forget(curr.fake_ref.clone());
            LocalData {
                // SAFETY: this is safe because we know that this will only ever be accessed
                // if there is a life reference to `self` present.
                parent: ManuallyDrop::new(unsafe { Arc::from_raw(Arc::as_ptr(self)) }),
                inner: UnsafeCell::new(LocalDataInner {
                    intermediate: LocalCounted { val: MaybeUninit::uninit(), ptr: null(), ref_cnt: 0, },
                    new: LocalCounted { val: MaybeUninit::uninit(), ptr: null(), ref_cnt: 0, },
                    curr: LocalCounted { val: MaybeUninit::new(D::from(curr_ptr)), ptr: curr_ptr, ref_cnt: 1, }
                }),
            }
        });
        // SAFETY: This is safe because we know that we are the only thread that
        // is able to access the thread local data at this time and said data has to be initialized
        // and we also know, that the pointer has to be non-null
        let data = unsafe { parent.inner.get().as_mut().unwrap_unchecked() };
        if unlikely(new) {
            let fake_ref = ManuallyDrop::new(D::from(unsafe { data.curr.val.assume_init_ref() }.as_ptr()));
            return SwapArcIntermediateGuard {
                parent,
                fake_ref,
            };
        }
        if data.new.ref_cnt == 0 {
            let ptr = if Self::strip_metadata(parent.parent.ptr.load(Ordering::Acquire)) != data.curr.ptr.cast_mut() {
                let curr = self.clone().load_internal();
                // increase the strong reference count
                mem::forget(curr.fake_ref.clone());
                let new_ptr = curr.fake_ref.as_ptr();
                data.new = LocalCounted {
                    val: MaybeUninit::new(D::from(new_ptr)),
                    ptr: new_ptr,
                    ref_cnt: 1,
                };
                new_ptr
            } else {
                data.curr.ref_cnt += 1;
                unsafe { data.curr.val.assume_init_ref() }.as_ptr()
            };
            // FIXME: perform an update! - this is probably fixed
            let fake_ref = ManuallyDrop::new(D::from(ptr));
            return SwapArcIntermediateGuard {
                parent,
                // SAFETY: we know that this is safe because the ref count is non-zero
                fake_ref,
            };
        }
        if data.intermediate.ref_cnt == 0 {
            let intermediate = Self::strip_metadata(parent.parent.intermediate_ptr.load(Ordering::Acquire));
            // check if there is a new intermediate value and that the intermediate value has been verified to be usable
            let ptr = if intermediate != data.new.ptr {
                let loaded = parent.parent.intermediate_ref_cnt.fetch_add(1, Ordering::SeqCst);
                if loaded & Self::UPDATE == 0 {
                    let loaded = Self::strip_metadata(parent.parent.intermediate_ptr.load(Ordering::Acquire));
                    data.intermediate = LocalCounted {
                        val: MaybeUninit::new(D::from(loaded)),
                        ptr: loaded,
                        ref_cnt: 1,
                    };
                    loaded
                } else {
                    parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                    data.new.ref_cnt += 1;
                    data.new.ptr
                }
            } else {
                data.new.ref_cnt += 1;
                data.new.ptr
            };
            let fake_ref = ManuallyDrop::new(D::from(ptr));
            return SwapArcIntermediateGuard {
                parent,
                fake_ref,
            };
        } else {
            println!("create intermediate ref!"); // FIXME: this and "delete intermediate ref" get spammed one after the other very fast
            data.intermediate.ref_cnt += 1;
            let fake_ref = ManuallyDrop::new(D::from(data.intermediate.ptr));
            return SwapArcIntermediateGuard {
                parent: &parent,
                fake_ref,
            };
        }
    }

    pub fn load_full(self: &Arc<Self>) -> D {
        self.load().as_ref().clone()
    }

    pub unsafe fn load_raw<'a>(self: &'a Arc<Self>) -> SwapArcIntermediatePtrGuard<'a, T, D, METADATA_PREFIX_BITS> {
        let guard = ManuallyDrop::new(self.load());
        let curr_meta = {
            Self::get_metadata(self.intermediate_ptr.load(Ordering::Acquire)) // FIXME: is this okay - even, when `intermediate` is getting updated?
        };
        SwapArcIntermediatePtrGuard {
            parent: guard.parent,
            ptr: guard.fake_ref.as_ptr().map_addr(|x| x | curr_meta),
        }
    }

    pub unsafe fn load_raw_full(self: &Arc<Self>) -> SwapArcIntermediateFullPtrGuard<T, D, METADATA_PREFIX_BITS> {
        let guard = self.load();
        let curr_meta = {
            Self::get_metadata(self.intermediate_ptr.load(Ordering::Acquire)) // FIXME: is this okay - even, when `intermediate` is getting updated?
        };
        SwapArcIntermediateFullPtrGuard {
            parent: guard.as_ref().clone(),
            ptr: guard.fake_ref.as_ptr().map_addr(|x| x | curr_meta),
        }
    }

    fn load_internal(self: Arc<Self>) -> SwapArcIntermediateInternalGuard<T, D, METADATA_PREFIX_BITS> {
        let ref_cnt = self.curr_ref_cnt.fetch_add(1, Ordering::SeqCst);
        let (ptr, src) = if ref_cnt & Self::UPDATE != 0 {
            let intermediate_ref_cnt = self.intermediate_ref_cnt.fetch_add(1, Ordering::SeqCst);
            if intermediate_ref_cnt & Self::UPDATE != 0 {
                let ret = self.ptr.load(Ordering::Acquire);
                // release the redundant reference
                self.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                (ret, RefSource::Curr)
            } else {
                let ret = self.intermediate_ptr.load(Ordering::Acquire);
                // release the redundant reference
                self.curr_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                (ret, RefSource::Intermediate)
            }
        } else {
            (self.ptr.load(Ordering::Acquire), RefSource::Curr)
        };
        // create a fake reference to the Arc to ensure so that the borrow checker understands
        // that the reference returned from the guard will point to valid memory
        let fake_ref = ManuallyDrop::new(D::from(Self::strip_metadata(ptr)));
        SwapArcIntermediateInternalGuard {
            parent: self,
            fake_ref,
            ref_src: src,
        }
    }

    fn try_update_curr(&self) -> bool {
        match self.curr_ref_cnt.compare_exchange(0, Self::UPDATE, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => {
                // FIXME: can we somehow bypass intermediate if we have a new update upcoming - we probably can't because this would probably cause memory leaks and other funny things that we don't like
                let intermediate = self.intermediate_ptr.load(Ordering::SeqCst);
                // update the pointer
                let prev = self.ptr.load(Ordering::Acquire);
                if Self::strip_metadata(prev) != Self::strip_metadata(intermediate) {
                    self.ptr.store(intermediate, Ordering::Release);
                    // unset the update flag
                    self.curr_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                    // unset the `weak` update flag from the intermediate ref cnt
                    self.intermediate_ref_cnt.fetch_and(!Self::OTHER_UPDATE, Ordering::SeqCst); // FIXME: are we sure this can't happen if there is UPDATE set for intermediate_ref?
                    // drop the `virtual reference` we hold to the Arc
                    D::from(Self::strip_metadata(prev));
                } else {
                    // unset the update flag
                    self.curr_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                }
                true
            }
            _ => false,
        }
    }

    fn try_update_intermediate(&self) {
        match self.intermediate_ref_cnt.compare_exchange(0, Self::UPDATE | Self::OTHER_UPDATE, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => {
                // take the update
                let update = self.updated.swap(null_mut(), Ordering::SeqCst);
                // check if we even have an update
                if !update.is_null() {
                    let metadata = Self::get_metadata(self.intermediate_ptr.load(Ordering::Acquire));
                    let update = Self::merge_ptr_and_metadata(update, metadata).cast_mut();
                    self.intermediate_ptr.store(update, Ordering::Release);
                    // unset the update flag
                    self.intermediate_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                    // try finishing the update up!
                    match self.curr_ref_cnt.compare_exchange(0, Self::UPDATE, Ordering::SeqCst, Ordering::Relaxed) {
                        Ok(_) => {
                            let prev = self.ptr.swap(update, Ordering::SeqCst);
                            // unset the update flag
                            self.curr_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                            // unset the `weak` update flag from the intermediate ref cnt
                            self.intermediate_ref_cnt.fetch_and(!Self::OTHER_UPDATE, Ordering::SeqCst);
                            // drop the `virtual reference` we hold to the Arc
                            D::from(Self::strip_metadata(prev));
                        }
                        Err(_) => {}
                    }
                } else {
                    // unset the update flags
                    self.intermediate_ref_cnt.fetch_and(!(Self::UPDATE | Self::OTHER_UPDATE), Ordering::SeqCst);
                }
            }
            Err(_) => {}
        }
    }

    pub fn update(&self, updated: D) {
        unsafe { self.update_raw(updated.as_ptr()); }
    }

    unsafe fn update_raw(&self, updated: *const T) {
        let updated = Self::strip_metadata(updated);
        let new = updated.cast_mut();
        let tmp = ManuallyDrop::new(D::from(new));
        tmp.increase_ref_cnt();
        loop {
            match self.intermediate_ref_cnt.compare_exchange(0, Self::UPDATE | Self::OTHER_UPDATE, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => {
                    // clear out old updates to make sure our update won't be overwritten by them in the future
                    let old = self.updated.swap(null_mut(), Ordering::SeqCst);
                    let metadata = Self::get_metadata(self.intermediate_ptr.load(Ordering::Acquire));
                    let new = Self::merge_ptr_and_metadata(new, metadata).cast_mut();
                    self.intermediate_ptr.store(new, Ordering::Release); // FIXME: this allows for metadata races - is this important? - we should at least document this properly!
                    // unset the update flag
                    self.intermediate_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                    if !old.is_null() {
                        // drop the `virtual reference` we hold to the Arc
                        D::from(old);
                    }
                    // try finishing the update up!
                    match self.curr_ref_cnt.compare_exchange(0, Self::UPDATE, Ordering::SeqCst, Ordering::Relaxed) {
                        Ok(_) => {
                            let prev = self.ptr.swap(new, Ordering::SeqCst);
                            // unset the update flag
                            self.curr_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                            // unset the `weak` update flag from the intermediate ref cnt
                            self.intermediate_ref_cnt.fetch_and(!Self::OTHER_UPDATE, Ordering::SeqCst);
                            // drop the `virtual reference` we hold to the Arc
                            D::from(Self::strip_metadata(prev)); // FIXME: this causes a use-after-free
                        }
                        Err(_) => {}
                    }
                    break;
                }
                Err(old) => {
                    if old & Self::UPDATE != 0 {
                        // somebody else already updates the current ptr, so we wait until they finish their update
                        continue;
                    }
                    // push our update up, so it will be applied in the future
                    let old = self.updated.swap(new, Ordering::SeqCst); // FIXME: should we add some sort of update counter
                    // FIXME: to determine which update is the most recent?
                    if !old.is_null() {
                        // drop the `virtual reference` we hold to the Arc
                        D::from(old);
                    }
                    break;
                }
            }
        }
    }

    unsafe fn try_compare_exchange<const IGNORE_META: bool>(&self, old: *const T, new: D/*&SwapArcIntermediateGuard<'_, T, D>*/) -> Result</*Option<*/bool/*>*/, D> {
        // FIXME: what should be compared against? `curr`? or is `update` to be taken into account as well?
        // FIXME: a good solution could be to compare against both `curr` and `intermediate` of the "main struct"(`ArcSwapIntermediateTLSLessFence`)
        // FIXME: this could lead to a problem tho because it doesn't seem very bulletproof to simply compare against 2 values that could point to two entirely different allocations
        // FIXME: if not done with careful consideration this could defeat the whole purpose of implementing compare_exchange for the data structure
        // FIXME: because this could lead to clients of this function to get unreliable feedback and weird updates to be performed which is the worst case scenario and should be avoided at all costs
        // FIXME: another solution could be to let the caller provide whether to check for the intermediate or curr value which is okay because then this
        // FIXME: compare_exchange function would check if the state the caller last saw is still up to date or not which would (probably) solve all problems
        // FIXME: we had before - the only problem that remains is that this will (probably) not work for thread locals because 1.
        // FIXME: their states are inconsistent across different threads and because something loaded as `RefSource::Curr` can become `RefSource::Intermediate`
        // FIXME: without notice - although if the first point could be neglected, the second one can be solved, the only problem remaining is that once
        // FIXME: we compared the thread local states, we have to perform a global update that is propagated down to the source of the compare
        // FIXME: if we do that we have replaced the global state source and have replaced the local one, the only problem remaining still
        // FIXME: is if another thread tries to perform a compare_exchange, but this could be solved by using compare_exchange on the global state source
        // FIXME: and acquiring the update lock. but if the update failed we have to somehow update the local state because otherwise we could
        // FIXME: run into an infinite loop (QUESTION: is this really true? because if we load the most recent value from local, we won't get an expired one -
        // FIXME: the only possible thing that i can think of right now is when intermediate is not null but even then intermediate should always contain
        // FIXME: the most recent value and there shouldn't be a more recent one in global state sources (except for `updated` - but only if that is to be
        // FIXME: considered a "global state source")
        // FIXME: also what is to be considered the "global state source"? is it `curr` or `updated` - or both in some weird way?
        // FIXME: if the local has some `intermediate` value then we can simply use said value for comparison (but only if the provided source is also `intermediate`)
        // FIXME: OR maybe even if not?
        if !self.intermediate_ref_cnt.compare_exchange(0, Self::UPDATE | Self::OTHER_UPDATE, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
            return Err(new);
        }
        let intermediate = self.intermediate_ptr.load(Ordering::Acquire);
        let cmp_result = if IGNORE_META {
            Self::strip_metadata(intermediate) == old
        } else {
            intermediate.cast_const() == old
        };
        if !cmp_result {
            self.intermediate_ref_cnt.fetch_and(!(Self::UPDATE | Self::OTHER_UPDATE), Ordering::SeqCst);
            return Ok(false);
        }
        // forget `new` in order to create a `virtual reference`
        let new = ManuallyDrop::new(new);
        let new = new.as_ptr();
        // clear out old updates to make sure our update won't be overwritten by them in the future
        let old_update = self.updated.swap(null_mut(), Ordering::SeqCst);
        let metadata = Self::get_metadata(intermediate);
        let new = Self::merge_ptr_and_metadata(new, metadata).cast_mut();
        self.intermediate_ptr.store(new, Ordering::Release);
        // unset the update flag
        self.intermediate_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
        if !old_update.is_null() {
            // drop the `virtual reference` we hold to the Arc
            D::from(old_update);
        }
        match self.curr_ref_cnt.compare_exchange(0, Self::UPDATE, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => {
                let prev = self.ptr.swap(new, Ordering::SeqCst);
                // unset the update flag
                self.curr_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                // unset the `weak` update flag from the intermediate ref cnt
                self.intermediate_ref_cnt.fetch_and(!Self::OTHER_UPDATE, Ordering::SeqCst);
                // drop the `virtual reference` we hold to the Arc
                D::from(Self::strip_metadata(prev));
            }
            Err(_) => {}
        }
        Ok(true)
    }

    // FIXME: this causes "deadlocks" if there are any other references alive
    unsafe fn try_compare_exchange_with_meta(&self, old: *const T, new: *const T/*&SwapArcIntermediateGuard<'_, T, D>*/) -> bool {
        let mut back_off_weight = 1;
        while !self.intermediate_ref_cnt.compare_exchange(0, Self::UPDATE | Self::OTHER_UPDATE, Ordering::SeqCst, Ordering::Relaxed).is_ok() {
            // back-off
            thread::sleep(Duration::from_micros(10 * back_off_weight));
            back_off_weight += 1;
            // println!("sleep: {back_off_weight}");
        }
        println!("inside cmp exchg!");
        let intermediate = self.intermediate_ptr.load(Ordering::Acquire);
        if intermediate.cast_const() != old {
            self.intermediate_ref_cnt.fetch_and(!(Self::UPDATE | Self::OTHER_UPDATE), Ordering::SeqCst);
            println!("cmp exchg failed!");
            return false;
        }
        // clear out old updates to make sure our update won't be overwritten by them in the future
        let old_update = self.updated.swap(null_mut(), Ordering::SeqCst);
        // increase the ref count
        let tmp = ManuallyDrop::new(D::from(Self::strip_metadata(new)));
        tmp.increase_ref_cnt();
        self.intermediate_ptr.store(new.cast_mut(), Ordering::Release);
        // unset the update flag
        self.intermediate_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
        if !old_update.is_null() {
            // drop the `virtual reference` we hold to the Arc
            D::from(old_update);
        }
        match self.curr_ref_cnt.compare_exchange(0, Self::UPDATE, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => {
                let prev = self.ptr.swap(new.cast_mut(), Ordering::SeqCst);
                // unset the update flag
                self.curr_ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
                // unset the `weak` update flag from the intermediate ref cnt
                self.intermediate_ref_cnt.fetch_and(!Self::OTHER_UPDATE, Ordering::SeqCst);
                // drop the `virtual reference` we hold to the Arc
                D::from(Self::strip_metadata(prev));
            }
            Err(_) => {}
        }
        println!("successfully finished cmp exchg!");
        true
    }

    pub fn update_metadata(&self, metadata: usize) {
        loop {
            let curr = self.intermediate_ptr.load(Ordering::Acquire);
            if self.try_update_meta(curr, metadata) { // FIXME: should this be a weak compare_exchange?
                break;
            }
        }
    }

    /// `old` should contain the previous metadata.
    pub fn try_update_meta(&self, old: *const T, metadata: usize) -> bool {
        let prefix = metadata & Self::META_MASK;
        self.intermediate_ptr.compare_exchange(old.cast_mut(), old.map_addr(|x| x | prefix).cast_mut(), Ordering::SeqCst, Ordering::Relaxed).is_ok()
    }

    pub fn set_in_metadata(&self, active_bits: usize) {
        self.intermediate_ptr.fetch_or(active_bits, Ordering::SeqCst);
    }

    pub fn unset_in_metadata(&self, inactive_bits: usize) {
        self.intermediate_ptr.fetch_and(!inactive_bits, Ordering::SeqCst);
    }

    pub fn load_metadata(&self) -> usize {
        self.intermediate_ptr.load(Ordering::Acquire).expose_addr() & Self::META_MASK
    }

    fn get_metadata(ptr: *const T) -> usize {
        ptr.expose_addr() & Self::META_MASK
    }

    fn strip_metadata(ptr: *const T) -> *const T {
        ptr.map_addr(|x| x & !Self::META_MASK)
    }

    fn merge_ptr_and_metadata(ptr: *const T, metadata: usize) -> *const T {
        ptr.map_addr(|x| x | metadata)
    }

    const META_MASK: usize = {
        let mut result = 0;
        let mut i = 0;
        while METADATA_PREFIX_BITS > i {
            result |= 1 << i;
            i += 1;
        }
        result
    };

    /// This will force an update, this means that new
    /// readers will have to wait for all old readers to
    /// finish and to update the ptr, even when no update
    /// is queued this will block new readers for a short
    /// amount of time, until failure got detected
    fn dummy() {}
    /*fn force_update(&self) -> UpdateResult {
        let curr = self.ref_cnt.fetch_or(Self::FORCE_UPDATE, Ordering::SeqCst);
        if curr & Self::UPDATE != 0 {
            return UpdateResult::AlreadyUpdating;
        }
        if self.updated.load(Ordering::SeqCst).is_null() {
            // unset the flag, as there are no upcoming updates
            self.ref_cnt.fetch_and(!Self::UPDATE, Ordering::SeqCst);
            return UpdateResult::NoUpdate;
        }
        UpdateResult::Ok
    }*/

}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Drop for SwapArcIntermediateTLS<T, D, METADATA_PREFIX_BITS> {
    fn drop(&mut self) {
        // FIXME: how should we handle intermediate inside drop?
        let updated = *self.updated.get_mut();
        if !updated.is_null() {
            D::from(updated);
        }
        let curr = Self::strip_metadata(*self.ptr.get_mut());
        let intermediate = Self::strip_metadata(*self.intermediate_ptr.get_mut());
        if intermediate != curr {
            // FIXME: the reason why we have to do this currently is because the update function doesn't work properly, fix the root cause!
            D::from(intermediate);
        }
        // drop the current arc
        D::from(curr);
    }
}

pub struct SwapArcIntermediatePtrGuard<'a, T, D: DataPtrConvert<T> = Arc<T>, const METADATA_PREFIX_BITS: u32 = 0> {
    parent: &'a LocalData<T, D, METADATA_PREFIX_BITS>,
    ptr: *const T,
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> SwapArcIntermediatePtrGuard<'_, T, D, METADATA_PREFIX_BITS> {

    #[inline]
    pub fn as_raw(&self) -> *const T {
        self.ptr
    }

}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Clone for SwapArcIntermediatePtrGuard<'_, T, D, METADATA_PREFIX_BITS> {
    fn clone(&self) -> Self {
        unsafe { self.parent.parent.load_raw() }
    }
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Drop for SwapArcIntermediatePtrGuard<'_, T, D, METADATA_PREFIX_BITS> {
    fn drop(&mut self) {
        // SAFETY: This is safe because we know that we are the only thread that
        // is able to access the thread local data at this time and said data has to be initialized
        // and we also know, that the pointer has to be non-null
        let data = unsafe { self.parent.inner.get().as_mut().unwrap_unchecked() };
        let ptr = SwapArcIntermediateTLS::<T, D, METADATA_PREFIX_BITS>::strip_metadata(self.ptr);
        // release the reference we hold
        if ptr == unsafe { data.curr.val.assume_init_ref() }.as_ptr() {
            data.curr.ref_cnt -= 1;
            if data.curr.ref_cnt == 0 {
                if !data.new.ptr.is_null() {
                    data.curr = mem::take(&mut data.new);
                    if !data.intermediate.ptr.is_null() {
                        data.new = mem::take(&mut data.intermediate).make_drop();
                        // increase the ref count of the new value
                        mem::forget(unsafe { data.new.val.assume_init_ref() }.clone());
                        self.parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            }
        } else if ptr == data.new.ptr {
            data.new.ref_cnt -= 1;
            if data.new.ref_cnt == 0 {
                if !data.intermediate.ptr.is_null() {
                    data.new = mem::take(&mut data.intermediate).make_drop();
                    // increase the ref count of the new value
                    mem::forget(unsafe { data.new.val.assume_init_ref() }.clone());
                    self.parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                }
            }
        } else if data.intermediate.ptr == ptr {
            println!("delete intermediate ref!");
            data.intermediate.ref_cnt -= 1;
            if data.intermediate.ref_cnt == 0 {
                self.parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
            }
        } else {
            panic!("1: found foreign ptr inside guard {:?}", ptr);
        }
    }
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Debug for SwapArcIntermediatePtrGuard<'_, T, D, METADATA_PREFIX_BITS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tmp = format!("{:?}", self.ptr);
        f.write_str(tmp.as_str())
    }
}

pub struct SwapArcIntermediateFullPtrGuard<T, D: DataPtrConvert<T> = Arc<T>, const METADATA_PREFIX_BITS: u32 = 0> {
    parent: D,
    ptr: *const T,
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> SwapArcIntermediateFullPtrGuard<T, D, METADATA_PREFIX_BITS> {

    #[inline]
    pub fn as_raw(&self) -> *const T {
        self.ptr
    }

}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Clone for SwapArcIntermediateFullPtrGuard<T, D, METADATA_PREFIX_BITS> {
    fn clone(&self) -> Self {
        Self {
            parent: self.parent.clone(),
            ptr: self.ptr,
        }
    }
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Debug for SwapArcIntermediateFullPtrGuard<T, D, METADATA_PREFIX_BITS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let tmp = format!("{:?}", self.ptr);
        f.write_str(tmp.as_str())
    }
}


pub struct SwapArcIntermediateGuard<'a, T, D: DataPtrConvert<T> = Arc<T>, const METADATA_PREFIX_BITS: u32 = 0> {
    parent: &'a LocalData<T, D, METADATA_PREFIX_BITS>,
    fake_ref: ManuallyDrop<D>,
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Drop for SwapArcIntermediateGuard<'_, T, D, METADATA_PREFIX_BITS> {
    fn drop(&mut self) {
        // SAFETY: This is safe because we know that we are the only thread that
        // is able to access the thread local data at this time and said data has to be initialized
        // and we also know, that the pointer has to be non-null
        let data = unsafe { self.parent.inner.get().as_mut().unwrap_unchecked() };
        // release the reference we hold
        if self.fake_ref.as_ptr() == data.curr.ptr {
            data.curr.ref_cnt -= 1;
            if data.curr.ref_cnt == 0 {
                if !data.new.ptr.is_null() {
                    data.curr = mem::take(&mut data.new);
                    if !data.intermediate.ptr.is_null() {
                        data.new = mem::take(&mut data.intermediate).make_drop();
                        // increase the ref count of the new value
                        mem::forget(unsafe { data.new.val.assume_init_ref() }.clone());
                        self.parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                    }
                }
            }
        } else if self.fake_ref.as_ptr() == data.new.ptr {
            data.new.ref_cnt -= 1;
            if data.new.ref_cnt == 0 {
                if !data.intermediate.ptr.is_null() {
                    data.new = mem::take(&mut data.intermediate).make_drop();
                    // increase the ref count of the new value
                    mem::forget(unsafe { data.new.val.assume_init_ref() }.clone());
                    self.parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                }
            }
        }  else if data.intermediate.ptr == self.fake_ref.as_ptr() {
            println!("delete intermediate ref!");
            data.intermediate.ref_cnt -= 1;
            if data.intermediate.ref_cnt == 0 {
                self.parent.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
            }
        } else {
            panic!("2: found foreign ptr inside guard {:?}", self.fake_ref.as_ptr());
        }
    }
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Deref for SwapArcIntermediateGuard<'_, T, D, METADATA_PREFIX_BITS> {
    type Target = D;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.fake_ref.deref()
    }
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Borrow<D> for SwapArcIntermediateGuard<'_, T, D, METADATA_PREFIX_BITS> {
    #[inline]
    fn borrow(&self) -> &D {
        self.fake_ref.deref()
    }
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> AsRef<D> for SwapArcIntermediateGuard<'_, T, D, METADATA_PREFIX_BITS> {
    #[inline]
    fn as_ref(&self) -> &D {
        self.fake_ref.deref()
    }
}

impl<T, D: DataPtrConvert<T> + Display, const METADATA_PREFIX_BITS: u32> Display for SwapArcIntermediateGuard<'_, T, D, METADATA_PREFIX_BITS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        D::fmt(self.as_ref(), f)
    }
}

impl<T, D: DataPtrConvert<T> + Debug, const METADATA_PREFIX_BITS: u32> Debug for SwapArcIntermediateGuard<'_, T, D, METADATA_PREFIX_BITS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        D::fmt(self.as_ref(), f)
    }
}


struct SwapArcIntermediateInternalGuard<T, D: DataPtrConvert<T> = Arc<T>, const METADATA_PREFIX_BITS: u32 = 0> {
    parent: Arc<SwapArcIntermediateTLS<T, D, METADATA_PREFIX_BITS>>,
    fake_ref: ManuallyDrop<D>,
    ref_src: RefSource,
}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Drop for SwapArcIntermediateInternalGuard<T, D, METADATA_PREFIX_BITS> {
    fn drop(&mut self) {
        // release the reference we hold
        match self.ref_src {
            RefSource::Curr => {
                let ref_cnt = self.parent.curr_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                if ref_cnt == 1 {
                    self.parent.try_update_curr();
                }
            }
            RefSource::Intermediate => {
                let ref_cnt = self.parent.intermediate_ref_cnt.fetch_sub(1, Ordering::SeqCst);
                // fast-rejection path to ensure we are only trying to update if it's worth it
                // FIXME: this probably isn't correct: Note: UPDATE is set (seldom) on the immediate ref_cnt if there is a forced update waiting in the queue
                if (ref_cnt == 1/* || ref_cnt == SwapArcIntermediate::<T>::UPDATE*/) && !self.parent.updated.load(Ordering::Acquire).is_null() {
                    self.parent.try_update_intermediate();
                }
            }
        }
    }
}

impl<T, D: DataPtrConvert<T> + Display, const METADATA_PREFIX_BITS: u32> Display for SwapArcIntermediateInternalGuard<T, D, METADATA_PREFIX_BITS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        D::fmt(self.fake_ref.deref(), f)
    }
}

impl<T, D: DataPtrConvert<T> + Debug, const METADATA_PREFIX_BITS: u32> Debug for SwapArcIntermediateInternalGuard<T, D, METADATA_PREFIX_BITS> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        D::fmt(self.fake_ref.deref(), f)
    }
}

enum RefSource {
    Curr,
    Intermediate,
}

struct LocalData<T, D: DataPtrConvert<T> = Arc<T>, const METADATA_PREFIX_BITS: u32 = 0> {
    parent: ManuallyDrop<Arc<SwapArcIntermediateTLS<T, D, METADATA_PREFIX_BITS>>>, // this acts as a reference with hidden lifetime that only we know is safe because
    // `parent` won't be used in the drop impl and `LocalData` can only be accessed
    // through `parent`
    inner: UnsafeCell<LocalDataInner<T, D>>,
}

// FIXME: add safety comment (the gist is that this will only ever be used when `parent` is dropped)
unsafe impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Sync for LocalData<T, D, METADATA_PREFIX_BITS> {}

impl<T, D: DataPtrConvert<T>, const METADATA_PREFIX_BITS: u32> Drop for LocalData<T, D, METADATA_PREFIX_BITS> {
    fn drop(&mut self) {
        println!("drop super");
    }
}

struct LocalDataInner<T, D: DataPtrConvert<T> = Arc<T>> {
    intermediate: LocalCounted<T, D>,
    new: LocalCounted<T, D, true>,
    curr: LocalCounted<T, D, true>,
}

struct LocalCounted<T, D: DataPtrConvert<T> = Arc<T>, const DROP: bool = false> {
    val: MaybeUninit<D>,
    ptr: *const T,
    ref_cnt: usize,
}

impl<T, D: DataPtrConvert<T>> LocalCounted<T, D, false> {

    fn make_drop(mut self) -> LocalCounted<T, D, true> {
        LocalCounted {
            val: mem::replace(&mut self.val, MaybeUninit::uninit()),
            ptr: self.ptr,
            ref_cnt: self.ref_cnt,
        }
    }

}

impl<T, D: DataPtrConvert<T>, const DROP: bool> Default for LocalCounted<T, D, DROP> {
    fn default() -> Self {
        Self {
            val: MaybeUninit::uninit(),
            ptr: null(),
            ref_cnt: 0,
        }
    }
}

// FIXME: is `Send` safe to implement even for `DROP = true`? - it probably is!
// FIXME: add safety comment (the gist is that this will only ever be used when `parent` is dropped)
unsafe impl<T, D: DataPtrConvert<T>, const DROP: bool> Send for LocalCounted<T, D, DROP> {}

impl<T, D: DataPtrConvert<T>, const DROP: bool> Drop for LocalCounted<T, D, DROP> {
    #[inline]
    fn drop(&mut self) {
        // println!("try dropping!");
        if DROP {
            // println!("dropping!");
            if !self.ptr.is_null() {
                // SAFETY: the person defining this struct has to make sure that
                // choosing `DROP` is correct.
                unsafe { self.val.assume_init_drop(); }
            }
        }
    }
}

/// SAFETY: Types implementing this trait are expected to perform
/// reference counting through cloning/dropping internally.
pub unsafe trait RefCnt: /*Send + Sync + */Clone {}

pub trait DataPtrConvert<T>: RefCnt + Sized {

    const INVALID: *const T;

    /// This function may not alter the reference count of the
    /// reference counted "object".
    fn from(ptr: *const T) -> Self;

    /// This function should decrement the reference count of the
    /// reference counted "object" indirectly, by automatically
    /// decrementing it on drop inside the "object"'s drop
    /// implementation.
    fn into(self) -> *const T;

    /// This function should NOT decrement the reference count of the
    /// reference counted "object" in any way, shape or form.
    fn as_ptr(&self) -> *const T;

    /// This function should increment the reference count of the
    /// reference counted "object" directly.
    fn increase_ref_cnt(&self);

}

unsafe impl<T> RefCnt for Arc<T> {}

impl<T> DataPtrConvert<T> for Arc<T> {
    const INVALID: *const T = null();

    fn from(ptr: *const T) -> Self {
        unsafe { Arc::from_raw(ptr) }
    }

    fn into(self) -> *const T {
        let ret = Arc::into_raw(self);
        // decrement the reference count
        <Self as DataPtrConvert<T>>::from(ret);
        ret
    }

    fn as_ptr(&self) -> *const T {
        Arc::as_ptr(self)
    }

    fn increase_ref_cnt(&self) {
        mem::forget(self.clone());
    }
}

unsafe impl<T> RefCnt for Option<Arc<T>> {}

impl<T> DataPtrConvert<T> for Option<Arc<T>> {
    const INVALID: *const T = null();

    fn from(ptr: *const T) -> Self {
        if !ptr.is_null() {
            Some(unsafe { Arc::from_raw(ptr) })
        } else {
            None
        }
    }

    fn into(self) -> *const T {
        match self {
            None => null(),
            Some(val) => {
                let ret = Arc::into_raw(val);
                // decrement the reference count
                <Self as DataPtrConvert<T>>::from(ret);
                ret
            },
        }
    }

    fn as_ptr(&self) -> *const T {
        match self {
            None => null(),
            Some(val) => Arc::as_ptr(val),
        }
    }

    fn increase_ref_cnt(&self) {
        mem::forget(self.clone());
    }
}

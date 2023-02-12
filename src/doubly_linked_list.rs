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
use std::fmt::{Debug, Display, Formatter};
use std::marker::PhantomData;
use std::pin::Pin;
use swap_arc::{SwapArc, SwapArcAny, SwapArcAnyMeta, SwapArcFullPtrGuard, SwapArcPtrGuard};

// disable MIRI SB(stacked borrows) checks

// this is better for cases where we DON'T care much about removal of nodes during traversal:
// https://www.codeproject.com/Articles/723555/A-Lock-Free-Doubly-Linked-List
// this is better if we DO:
// https://scholar.google.com/citations?view_op=view_citation&hl=de&user=RJmBj1wAAAAJ&citation_for_view=RJmBj1wAAAAJ:UebtZRa9Y70C

// FIXME: NOTE THAT: &mut T can be converted into *mut T by using .into() on the mutable reference!

// FIXME: add guard to nodes in Unbound mode to remove them once all of the references to them get dropped
// FIXME: employ reference counting on the nodes in order for them to be dropped correctly
#[derive(Clone)]
pub struct AtomicDoublyLinkedList<T: Send + Sync, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    header_node: Arc<RawNode<T, NODE_KIND>>,
    // in the header node itself, the left field points to the `header_node` field of the list itself, so we don't have to maintain a reference count
    tail_node: Arc<RawNode<T, NODE_KIND>>,
    // in the tail node itself, the right field points to the `tail_node` field of the list itself, so we don't have to maintain a reference count
}

impl<T: Send + Sync, const NODE_KIND: NodeKind> AtomicDoublyLinkedList<T, NODE_KIND> {
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
        /*
        if align_of::<Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND>>>>() < 4 {
            unreachable!("Arc's alignment isn't sufficient!");
        }*/
        let ret = Arc::new(Self {
            header_node: Arc::new(Aligned(AtomicDoublyLinkedListNode {
                val: MaybeUninit::uninit(),
                left: Link::invalid(),
                right: Link::invalid(),
            })),
            tail_node: Arc::new(Aligned(AtomicDoublyLinkedListNode {
                val: MaybeUninit::uninit(),
                left: Link::invalid(),
                right: Link::invalid(),
            })),
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
                let head = ManuallyDrop::new(unsafe { Arc::from_raw(head) });
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
                mem::forget(unsafe { Arc::from_raw(head) });
                let head = unsafe { Arc::from_raw(head) };
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
                    .get()
                    .get_ptr()
                    .cast::<RawNode<T, { NodeKind::Bound }>>();
                let tail = ManuallyDrop::new(unsafe { Arc::from_raw(tail) });
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
                    .get();
                let tail = tail
                    .get_ptr()
                    .cast::<RawNode<T, { NodeKind::Unbound }>>();
                let tail = unsafe { Arc::from_raw(tail) };
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

impl<T: Send + Sync, const NODE_KIND: NodeKind> Drop for AtomicDoublyLinkedList<T, NODE_KIND> {
    fn drop(&mut self) {
        // remove all nodes in the list, when the list gets dropped,
        // this makes sure that all the nodes' state is consistent
        // and correct
        println!("dropping list!");
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
    T: Send + Sync,
    const NODE_KIND: NodeKind = { NodeKind::Bound },
    const DELETED: bool = false,
> {
    // TODO: change the bool here into an enum!
    val: MaybeUninit<T>,
    left: Link<T, NODE_KIND>,
    right: Link<T, NODE_KIND>,
}

static COUNTER: AtomicUsize = AtomicUsize::new(0);

impl<T: Send + Sync> AtomicDoublyLinkedListNode<T, { NodeKind::Bound }, false> {
    pub fn remove(self: &Arc<Aligned<A4, Self>>) -> Option<Node<T, { NodeKind::Bound }, true>> {
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
            println!("pre set right!");
            // println!("right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
            let tmp = self.right.get();
            let tmp_right = ManuallyDrop::new(unsafe { Arc::from_raw(tmp.get_ptr()) });
            // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
            println!("right arc refs: {}", Arc::strong_count(&tmp_right));
            drop(tmp);
            println!("curr right: {:?}", self.right.get().raw_ptr());
            println!("next: {:?}", next.raw_ptr());
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
                println!("pre head stuff!");
                // println!("right: curr refs {} | intermediate refs: {}", self.right.ptr.curr_ref_cnt.load(Ordering::SeqCst), self.right.ptr.intermediate_ref_cnt.load(Ordering::SeqCst));
                let tmp = self.right.get();
                let tmp_right = ManuallyDrop::new(unsafe { Arc::from_raw(tmp.get_ptr()) });
                // let tmp_left = ManuallyDrop::new(unsafe { Arc::from_raw(self.left.get().get_ptr().as_ref().unwrap()) });
                println!("right arc refs: {}", Arc::strong_count(&tmp_right));
                drop(tmp);
                let prev_tmp = ManuallyDrop::new(unsafe { Arc::from_raw(prev.get_ptr()) });
                println!("left pre ptr: {:?}", self.left.get().raw_ptr());
                println!("right pre ptr: {:?}", self.right.get().raw_ptr());
                if let PtrGuardOrPtr::FullGuard(guard) = prev_tmp
                    .correct_prev::<true>(/*leak_arc(unsafe { Arc::from_raw(next.get_ptr()) })*/next.get_ptr()) { // FIXME: PROBABLY: next is already freed when it gets derefed again
                    prev = FullLinkContent {
                        ptr: guard, // FIXME: we get an unwrapped none panic in this line (in Bound mode) - that's probably because here we have a header node which we try to deref!
                    };
                }
                println!("post head stuff!");
                println!("left post ptr: {:?}", self.left.get().raw_ptr());
                println!("right post ptr: {:?}", self.right.get().raw_ptr());
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

impl<T: Send + Sync> AtomicDoublyLinkedListNode<T, { NodeKind::Unbound }, false> {
    pub fn remove(self: Arc<Aligned<A4, Self>>) -> Option<Node<T, { NodeKind::Bound }, true>> {
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

impl<T: Send + Sync, const NODE_KIND: NodeKind> AtomicDoublyLinkedListNode<T, NODE_KIND, false> {
    pub fn add_after(self: &Arc<Aligned<A4, Self>>, val: T) -> Node<T, NODE_KIND> {
        let node = Arc::new/*pin*/(Aligned(AtomicDoublyLinkedListNode {
            val: MaybeUninit::new(val),
            left: Link::invalid(),
            right: Link::invalid(),
        }));

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
        // let this = Arc::as_ptr(self);
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
            println!("in loop!");
            next = self.right.get_full()/*self.right.get()*/; // = tail // FIXME: MIRI flags this because apparently the SwapArc here gets dropped while performing the `get_full` operation.
            println!("got full!");
            unsafe {
                // create_ref(this); // store_ref(node.left, <prev, false>) | in iter 1: head refs += 1

                // increase the ref count by 1
                // mem::forget(self.clone());
                // create_ref(this);
                // node.left.set_unsafe/*::<false>*/(this);
            }
            unsafe {
                // create_ref(next.get_ptr()); // store_ref(node.right, <next, false>) | in iter 1: tail refs += 1

                // increase the ref count by 1
                // create_ref(next.get_ptr());
                // node.right.set_unsafe/*::<false>*/(next.get_ptr());
            }

            /*println!("set meta!");
            if self
                .right
                .try_set_addr_full_with_meta(next.get_ptr(), Arc::as_ptr(node))
                // .try_set_addr_full/*::<false>*/(next.get_ptr(), node.clone())
            {
                // SAFETY: we have to create a reference to node in order ensure, that it is always valid, when it has to be
                // unsafe { create_ref(Arc::as_ptr(node)); }
                break;
            }*/
            // unsafe { release_ref(next.get_ptr()); } // in iter 1: tail refs -= 1
            /*if self.right.get_meta().get_deletion_marker() { // FIXME: err when using get() instead of get_meta()
                // release_ref(node); // this is probably unnecessary, as we don't delete the node, but reuse it
                // delete_node(node);
                println!("adding beffffore!");
                return self.inner_add_before(node);
            }*/
            // back-off
            thread::sleep(Duration::from_micros(10 * back_off_weight));
            back_off_weight += 1;
        }
        println!("added after!");
        // *cursor = node;
        // let _/*prev*/ = self.correct_prev::<false>(next.get_ptr()); // FIXME: this isn't even called until it panics, so this can't be the cause!
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
        let node = Arc::new/*pin*/(Aligned(AtomicDoublyLinkedListNode {
            val: MaybeUninit::new(val),
            left: Link::invalid(),
            right: Link::invalid(),
        }));

        self.inner_add_before(&node);

        if NODE_KIND == NodeKind::Bound {
            let ret = node.clone();
            let _ = ManuallyDrop::new(ret); // leak a single reference
        }
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
                    != ptr::from_exposed_addr(next.raw_ptr().expose_addr() | DELETION_MARKER)
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
    ) -> PtrGuardOrPtr<'a, RawNode<T, NODE_KIND>> {
        println!("start correct prev!");
        let node = ManuallyDrop::new(unsafe { Arc::from_raw(node) });
        // let id = rand::random::<usize>();
        // FIXME: currently there is an issue where we go further and further to the right without finding from `self` without finding the `node` we are looking for
        // let initial = Arc::as_ptr(self); // = node
        let mut back_off_weight = 1;
        let mut prev = PtrGuardOrPtr::Ptr(Arc::as_ptr(self)); // = node
        let mut last_link: Option<PtrGuardOrPtr<'_, RawNode<T, NODE_KIND>>/*NodePtr<T, NODE_KIND>*/> = None;
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

impl<T: Send + Sync, const NODE_KIND: NodeKind, const DELETED: bool>
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

impl<T: Send + Sync, const NODE_KIND: NodeKind, const DELETED: bool> Drop
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
struct Link<T: Send + Sync, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    // ptr: Aligned<A4, AtomicPtr<RawNode<T, NODE_KIND>>>,
    ptr: Aligned<A4, Arc<SwapArcAnyMeta<(), Option<Arc<()>>, 0>>/*Arc<SwapArcAnyMeta<RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>/*Option<Arc<RawNode<T, NODE_KIND>>>*/, 2>>*/>,
    _phantom_data: PhantomData<T>,
}

impl<T: Send + Sync, const NODE_KIND: NodeKind> Link<T, NODE_KIND> {
    const CAS_ORDERING: Ordering = Ordering::SeqCst;

    #[inline(always)]
    fn ptr_inner(&self) -> &Aligned<A4, Arc<SwapArcAnyMeta<RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>>> {
        unsafe { (&self.ptr as *const Aligned<A4, Arc<SwapArcAnyMeta<(), Option<Arc<()>>, 0>>>).cast::<Aligned<A4, Arc<SwapArcAnyMeta<RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>>>>().as_ref().unwrap_unchecked() }
    }

    fn get(&self) -> LinkContent<'_, T, NODE_KIND/*SwapArcIntermediatePtrGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>*//*, LinkContent<T, NODE_KIND>*/>/*LinkContent<T, NODE_KIND>*//*SwapArcIntermediateGuard<'_, LinkContent<T, NODE_KIND>>*/ {
        LinkContent {
            ptr: unsafe { self.ptr_inner().load_raw() },
        }
    }

    fn get_full(&self) -> FullLinkContent<T, NODE_KIND> {
        FullLinkContent {
            ptr: self.ptr_inner().load_raw_full(),
        }
    }

    /*
    fn get_typed(&self) -> SwapArcIntermediateGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2> {
        self.ptr.load()
    }*/

    fn get_meta(&self) -> Metadata {
        Metadata(self.ptr_inner().load_metadata())
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
        self.ptr_inner().set_in_metadata(DELETION_MARKER);
    }

    fn try_set_addr_full/*<const SET_DELETION_MARKER: bool>*/(
        &self,
        old: NodePtr<T, NODE_KIND>,
        new: Arc<RawNode<T, NODE_KIND>>/*&SwapArcIntermediateGuard<'_, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>,*/
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
        unsafe { self.ptr_inner().try_compare_exchange_ignore_meta(old, new.deref() as *const _) }
    }

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
        unsafe { self.ptr_inner().try_compare_exchange_with_meta(old, new) }
    }

    fn try_set_deletion_marker/*<const SET_DELETION_MARKER: bool>*/(&self, old: NodePtr<T, NODE_KIND>) -> bool {
        // self.try_set_addr_full::<true/*SET_DELETION_MARKER*/>(old, old)
        self.ptr_inner().try_update_meta(old, DELETION_MARKER)
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
        self.ptr_inner().store_raw(new);



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
            ptr: Aligned(Arc::new(SwapArcAnyMeta::new(None))),
            _phantom_data: Default::default(),
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
struct LinkContent<'a, T: Send + Sync, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    // ptr: NodePtr<T, NODE_KIND>,
    ptr: SwapArcPtrGuard<'a, RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>,
}

impl<T: Send + Sync, const NODE_KIND: NodeKind> LinkContent<'_, T, NODE_KIND> {
    fn get_deletion_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DELETION_MARKER) != 0
    }

    fn get_detached_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DETACHED_MARKER) != 0
    }

    fn get_ptr(&self) -> NodePtr<T, NODE_KIND> {
        ptr::from_exposed_addr_mut(
            self.raw_ptr().expose_addr() & (!(DELETION_MARKER | DETACHED_MARKER)),
        )
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

impl<T: Send + Sync, const NODE_KIND: NodeKind> PartialEq for LinkContent<'_, T, NODE_KIND> {
    fn eq(&self, other: &Self) -> bool {
        self.raw_ptr().expose_addr() == other.raw_ptr().expose_addr()
    }
}

// FIXME: always pass this around wrapped inside a guard!
// #[derive(Copy, Clone, Debug)]
struct FullLinkContent<T: Send + Sync, const NODE_KIND: NodeKind = { NodeKind::Bound }> {
    // ptr: NodePtr<T, NODE_KIND>,
    ptr: SwapArcFullPtrGuard<RawNode<T, NODE_KIND>, Option<Arc<RawNode<T, NODE_KIND>>>, 2>,
}

impl<T: Send + Sync, const NODE_KIND: NodeKind> FullLinkContent<T, NODE_KIND> {
    fn get_deletion_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DELETION_MARKER) != 0
    }

    fn get_detached_marker(&self) -> bool {
        (self.raw_ptr().expose_addr() & DETACHED_MARKER) != 0
    }

    fn get_ptr(&self) -> NodePtr<T, NODE_KIND> {
        ptr::from_exposed_addr_mut(
            self.raw_ptr().expose_addr() & (!(DELETION_MARKER | DETACHED_MARKER)),
        )
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

enum PtrGuardOrPtr<'a, T: Send + Sync> {
    Guard(SwapArcPtrGuard<'a, T, Option<Arc<T>>, 2>),
    FullGuard(SwapArcFullPtrGuard<T, Option<Arc<T>>, 2>),
    Ptr(*const T),
}

impl<T: Send + Sync> PtrGuardOrPtr<'_, T> {

    const META_MASK: usize = ((1 << 0) | (1 << 1));

    fn as_ptr(&self) -> *const T {
        match self {
            PtrGuardOrPtr::Guard(guard) => guard.as_raw(),
            PtrGuardOrPtr::Ptr(ptr) => *ptr,
            PtrGuardOrPtr::FullGuard(full_guard) => full_guard.as_raw(),
        }
    }

    fn as_ptr_no_meta(&self) -> *const T {
        ptr::from_exposed_addr(self.as_ptr().expose_addr() & !Self::META_MASK)
    }

}

unsafe fn release_ref<T: Send + Sync, const NODE_KIND: NodeKind>(node_ptr: NodePtr<T, NODE_KIND>) {
    // decrement the reference count of the arc
    Arc::from_raw(node_ptr);
}

unsafe fn release_ref_2<T: Send + Sync, const NODE_KIND: NodeKind>(node_ptr: NodePtr<T, NODE_KIND>, node_ptr_2: NodePtr<T, NODE_KIND>) {
    // FIXME: is this the correct handling of release_ref with 2 params?
    // release_ref(node_ptr);
    // release_ref(node_ptr_2);
}

unsafe fn create_ref<T: Send + Sync, const NODE_KIND: NodeKind>(node_ptr: NodePtr<T, NODE_KIND>) {
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

type NodePtr<T, const NODE_KIND: NodeKind, const REMOVED: bool = false> =
    *const RawNode<T, NODE_KIND, REMOVED>;
type RawNode<T, const NODE_KIND: NodeKind, const REMOVED: bool = false> =
    Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, REMOVED>>;
pub type Node<T, const NODE_KIND: NodeKind, const REMOVED: bool = false> =
    /*Pin<*/Arc<Aligned<A4, AtomicDoublyLinkedListNode<T, NODE_KIND, REMOVED>>>/*>*/;

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
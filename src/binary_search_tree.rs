use std::alloc::{dealloc, Layout};
use std::{mem, ptr};
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

pub struct BinarySearchTree<K: PartialOrd, T> {
    val: Option<NonNull<TreeNode<K, T>>>,
}

impl<K: PartialOrd, T> BinarySearchTree<K, T> {

    #[inline(always)]
    pub fn new() -> Self {
        Self {
            val: None,
        }
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.is_empty()
    }

    pub fn insert(&mut self, key: K, val: T) -> bool {

    }

    pub fn remove(&mut self, key: &K) -> Option<T> {
        match &mut self.val {
            None => None,
            Some(val) => {
                let val_ref = unsafe { val.as_ref() };
                if &val_ref.key == key {
                    let (key, value) = unsafe { TreeNode::remove_node(&mut self.val) };
                }
            }
        }
    }

    // FIXME: implement get_range and iter

    pub fn get(&self, key: &K) -> Option<&T> {

    }

    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

}

struct TreeNode<K: PartialOrd, T> {
    left: Option<NonNull<TreeNode<K, T>>>,
    right: Option<NonNull<TreeNode<K, T>>>,
    key: K,
    val: T,
}

impl<K: PartialOrd, T> TreeNode<K, T> {

    fn remove(&mut self, key: &K) -> Option<T> {
        if let Some(left) = &mut self.left {
            let left_ref = unsafe { left.as_mut() };
            if &left_ref.key == key {
                let (key, value) = unsafe { Self::remove_node(left) }.unwrap();
                self.key = key;
                let ret = mem::replace(&mut self.val, value);
                return Some(ret);
            } else {
                return left_ref.remove(key);
            }
        }
        if let Some(right) = &mut self.right {
            let right_ref = unsafe { right.as_mut() };
            if &right_ref.key == key {
                let (key, value) = unsafe { Self::remove_node(right) }.unwrap();
                self.key = key;
                let ret = mem::replace(&mut self.val, value);
                return Some(ret);
            } else {
                return right_ref.remove(key);
            }
        }

        None
    }

    unsafe fn destroy_children(&self) {
        if let Some(left) = self.left {
            unsafe { left.as_ptr().drop_in_place(); }
            unsafe { dealloc(left.cast::<u8>().as_ptr(), Layout::new::<TreeNode<K, T>>()); }
        }
        if let Some(right) = self.right {
            unsafe { right.as_ptr().drop_in_place(); }
            unsafe { dealloc(right.cast::<u8>().as_ptr(), Layout::new::<TreeNode<K, T>>()); }
        }
    }

    unsafe fn remove_node(slf: &mut Option<NonNull<Self>>) -> Option<(K, T)> {
        match slf {
            None => None,
            Some(node) => {
                let node = unsafe { node.as_mut() };
                if let Some(left) = node.left {
                    if node.right.is_none() {
                        // replace the entire node because we have no other subtree(child)
                        let val = ManuallyDrop::new(unsafe { (&node.val as *const T).read() });
                        let key = ManuallyDrop::new(unsafe { (&node.key as *const K).read() });
                        mem::forget(mem::replace(node, unsafe { left.as_ptr().read() }));
                        return Some((ManuallyDrop::into_inner(val), ManuallyDrop::into_inner(key)));
                    }
                    let has_no_children = node.left.unwrap().as_ref().left.is_none() && node.left.unwrap().as_ref().right.is_none();
                    let (other_val, other_key) = unsafe { Self::remove_node(&mut node.left) }.unwrap();
                    let val = mem::replace(&mut node.val, other_val);
                    let key = mem::replace(&mut node.key, other_key);
                    if has_no_children {
                        node.left = None;
                    }
                    // we can keep our left node as our child as its value was already changed by the remove_node call
                    // FIXME: do this differently as this has many memcpys (but it keeps the order of pointers in memory which is an important property we need to keep for the concurrent version)
                    return Some((key, val));
                }

                if let Some(right) = node.right {
                    // at this point we know that the left node has to be None

                    // replace the entire node because we have no other subtree(child)
                    let val = ManuallyDrop::new(unsafe { (&node.val as *const T).read() });
                    let key = ManuallyDrop::new(unsafe { (&node.key as *const K).read() });
                    mem::forget(mem::replace(node, unsafe { right.as_ptr().read() }));
                    return Some((ManuallyDrop::into_inner(key), ManuallyDrop::into_inner(val)));
                }


                // if this node has no children then this ptr would point to a nil node, so we have to set it to none.
                *slf = None;

                let val = ManuallyDrop::new(unsafe { (&node.val as *const T).read() });
                let key = ManuallyDrop::new(unsafe { (&node.key as *const K).read() });

                Some()
            }
        }
    }

}

impl<K: PartialOrd, T> Drop for TreeNode<K, T> {
    fn drop(&mut self) {
        unsafe { self.destroy_children(); }
    }
}
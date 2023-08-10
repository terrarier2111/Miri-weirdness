use std::alloc::{alloc, dealloc, Layout};
use std::{mem, ptr};
use std::cmp::Ordering;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

pub struct BinarySearchTree<K: Ord, T> {
    val: Option<NonNull<TreeNode<K, T>>>,
}

impl<K: Ord, T> BinarySearchTree<K, T> {

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

    pub fn insert(&mut self, key: K, val: T) {
        match &mut self.val {
            None => {
                let node = unsafe { alloc(Layout::new::<TreeNode<K, T>>()) }.cast::<TreeNode<K, T>>();
                if !node.is_null() {
                    unsafe {
                        node.write(TreeNode {
                            left: None,
                            right: None,
                            key,
                            val,
                        })
                    };
                    self.val = Some(unsafe { NonNull::new_unchecked(node) });
                }
            }
            Some(node) => {
                unsafe { node.as_mut() }.insert(key, val);
            }
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<T> {
        match &mut self.val {
            None => None,
            Some(val) => {
                let val_ref = unsafe { val.as_mut() };
                if &val_ref.key == key {
                    let (_, value) = unsafe { TreeNode::remove_node(&mut self.val) }.unwrap();
                    Some(value)
                } else {
                    val_ref.remove(key)
                }
            }
        }
    }

    // FIXME: implement get_range and iter

    pub fn get(&self, key: &K) -> Option<&T> {
        self.val.map(|node| unsafe { node.as_ref() }.get(key)).flatten()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

}

impl<K: Ord, T> Drop for BinarySearchTree<K, T> {
    fn drop(&mut self) {
        if let Some(node) = self.val {
            unsafe { node.as_ptr().drop_in_place(); }
            unsafe { dealloc(node.as_ptr().cast::<u8>(), Layout::new::<TreeNode<K, T>>()); }
        }
    }
}

struct TreeNode<K: Ord, T> {
    left: Option<NonNull<TreeNode<K, T>>>,
    right: Option<NonNull<TreeNode<K, T>>>,
    key: K,
    val: T,
}

impl<K: Ord, T> TreeNode<K, T> {

    fn insert(&mut self, key: K, value: T) {
        match self.key.cmp(&key) {
            Ordering::Less => {
                if let Some(mut node) = self.left {
                    unsafe { node.as_mut() }.insert(key, value);
                } else {
                    let node = unsafe { alloc(Layout::new::<TreeNode<K, T>>()) }.cast::<TreeNode<K, T>>();
                    if !node.is_null() {
                        unsafe {
                            node.write(TreeNode {
                                left: None,
                                right: None,
                                key,
                                val: value,
                            })
                        };
                        self.left = Some(unsafe { NonNull::new_unchecked(node) });
                    }
                }
            },
            Ordering::Equal => panic!("Tried to insert a key that is already present"),
            Ordering::Greater => {
                if let Some(mut node) = self.right {
                    unsafe { node.as_mut() }.insert(key, value);
                } else {
                    let node = unsafe { alloc(Layout::new::<TreeNode<K, T>>()) }.cast::<TreeNode<K, T>>();
                    if !node.is_null() {
                        unsafe {
                            node.write(TreeNode {
                                left: None,
                                right: None,
                                key,
                                val: value,
                            })
                        };
                        self.right = Some(unsafe { NonNull::new_unchecked(node) });
                    }
                }
            }
        }
    }

    fn remove(&mut self, key: &K) -> Option<T> {
        if let Some(left) = &mut self.left {
            let left_ref = unsafe { left.as_mut() };
            if &left_ref.key == key {
                let (key, value) = unsafe { Self::remove_node(&mut self.left) }.unwrap();
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
                let (key, value) = unsafe { Self::remove_node(&mut self.right) }.unwrap();
                self.key = key;
                let ret = mem::replace(&mut self.val, value);
                return Some(ret);
            } else {
                return right_ref.remove(key);
            }
        }

        None
    }

    fn get(&self, key: &K) -> Option<&T> {
        if &self.key == key {
            return Some(&self.val);
        }
        if let Some(val) = self.left.map(|node| unsafe { node.as_ref() }.get(key)).flatten() {
            return Some(val);
        }
        self.right.map(|node| unsafe { node.as_ref() }.get(key)).flatten()
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
                        return Some((ManuallyDrop::into_inner(key), ManuallyDrop::into_inner(val)));
                    }
                    let has_no_children = node.left.unwrap().as_ref().left.is_none() && node.left.unwrap().as_ref().right.is_none();
                    let (other_key, other_val) = unsafe { Self::remove_node(&mut node.left) }.unwrap();
                    let val = mem::replace(&mut node.val, other_val);
                    let key = mem::replace(&mut node.key, other_key);
                    if has_no_children {
                        // FIXME: is this even necessary?
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

                let val = ManuallyDrop::new(unsafe { (&node.val as *const T).read() });
                let key = ManuallyDrop::new(unsafe { (&node.key as *const K).read() });

                // FIXME: free memory backing node
                // if this node has no children then this ptr would point to a nil node, so we have to set it to none.
                mem::forget(slf.take());

                Some((ManuallyDrop::into_inner(key), ManuallyDrop::into_inner(val)))
            }
        }
    }

}

impl<K: Ord, T> Drop for TreeNode<K, T> {
    fn drop(&mut self) {
        unsafe { self.destroy_children(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert() {
        let mut tree = BinarySearchTree::new();
        for i in 0..20 {
            tree.insert(i, format!("test {}", i));
            assert_eq!(tree.get(&i), Some(&format!("test {}", i)));
            assert_eq!(tree.get(&(i + 1)), None);
        }
        for i in 0..20 {
            assert_eq!(tree.get(&i), Some(&format!("test {}", i)));
            assert_eq!(tree.get(&(1 + 20)), None);
        }
    }

    #[test]
    fn remove() {
        let mut tree = BinarySearchTree::new();
        for i in 0..20 {
            tree.insert(i, format!("test {}", i));
        }
        for i in 0..20 {
            assert_eq!(tree.remove(&i), Some(format!("test {}", i)));
            assert_eq!(tree.get(&i), None);
        }
    }

}

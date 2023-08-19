use std::alloc::{alloc, dealloc, Layout};
use std::{mem, ptr};
use std::cmp::Ordering;
use std::fmt::{Debug, Formatter, Pointer, Write};
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

pub struct RbTree<K: Ord, T> {
    val: Option<NonNull<TreeNode<K, T>>>,
}

impl<K: Ord, T> RbTree<K, T> {

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
                            parent: None,
                            left: None,
                            right: None,
                            color: Color::Black,
                            key,
                            val,
                        })
                    };
                    self.val = Some(unsafe { NonNull::new_unchecked(node) });
                }
            }
            Some(node) => {
                TreeNode::insert(*node, &mut self.val as *mut _, key, val);
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

impl<K: Ord, T> Drop for RbTree<K, T> {
    fn drop(&mut self) {
        if let Some(node) = self.val {
            unsafe { node.as_ptr().drop_in_place(); }
            unsafe { dealloc(node.as_ptr().cast::<u8>(), Layout::new::<TreeNode<K, T>>()); }
        }
    }
}

impl<K: Ord + Debug, T: Debug> Debug for RbTree<K, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(node) = self.val {
            unsafe { node.as_ref() }.fmt(f)?;
        }
        Ok(())
    }
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
#[repr(u8)]
enum Color {
    Red = 0,
    Black = 1,
}

#[derive(Eq, PartialEq, Copy, Clone, Debug)]
enum Direction {
    Left,
    Right,
}

impl Direction {

    #[inline]
    fn rev(self) -> Self {
        match self {
            Direction::Left => Direction::Right,
            Direction::Right => Direction::Left,
        }
    }

}

struct TreeNode<K: Ord, T> {
    parent: Option<NonNull<TreeNode<K, T>>>,
    left: Option<NonNull<TreeNode<K, T>>>,
    right: Option<NonNull<TreeNode<K, T>>>,
    color: Color,
    key: K,
    val: T,
}

impl<K: Ord, T> TreeNode<K, T> {

    #[inline]
    fn get_sibling(&self, child: NonNull<TreeNode<K, T>>) -> Option<NonNull<TreeNode<K, T>>> {
        if self.left == Some(child) {
            return self.right;
        }
        assert_eq!(self.right, Some(child));
        self.left
    }

    #[inline]
    fn child_dir(&self, child: NonNull<TreeNode<K, T>>) -> Direction {
        if self.left == Some(child) {
            return Direction::Left;
        }
        assert_eq!(self.right, Some(child));
        Direction::Right
    }

    fn rot_left_par(&mut self) {
        let left = self.left;
        self.left = self.parent;
        if let Some(mut left) = left {
            unsafe { left.as_mut() }.parent = self.parent;
            println!("rl 0");
        }
        if let Some(mut parent) = self.parent {
            let this = unsafe { NonNull::new_unchecked(self as *mut TreeNode<K, T>) };
            assert_eq!(unsafe { parent.as_ref() }.child_dir(this), Direction::Right);
            unsafe { parent.as_mut() }.right = left;
            let new_parent = unsafe { parent.as_ref() }.parent;
            unsafe { parent.as_mut() }.parent = Some(this);
            self.parent = new_parent;
            if let Some(mut new_parent) = new_parent {
                if unsafe { new_parent.as_ref() }.child_dir(parent) == Direction::Left {
                    unsafe { new_parent.as_mut().left = Some(this); }
                } else {
                    unsafe { new_parent.as_mut().right = Some(this); }
                }
            }
            println!("rl 1 {:?}", new_parent);
        } else {
            self.parent = None;
            assert!(left.is_none());
            println!("rl 2");
        }
    }

    fn rot_right_par(&mut self) {
        let right = self.right;
        self.right = self.parent;
        if let Some(mut right) = right {
            unsafe { right.as_mut() }.parent = self.parent;
        }
        if let Some(mut parent) = self.parent {
            unsafe { parent.as_mut() }.left = right;
            let new_parent = unsafe { parent.as_ref() }.parent;
            unsafe { parent.as_mut() }.parent = Some(unsafe { NonNull::new_unchecked(self as *mut TreeNode<K, T>) });
            self.parent = new_parent;
        }
    }

    fn insert(mut slf: NonNull<Self>, root: *mut Option<NonNull<TreeNode<K, T>>>, key: K, value: T) {
        match unsafe { slf.as_ref() }.key.cmp(&key) {
            Ordering::Less => {
                if let Some(node) = unsafe { slf.as_ref() }.right {
                    Self::insert(node, root, key, value);
                } else {
                    let node_raw = unsafe { alloc(Layout::new::<TreeNode<K, T>>()) }.cast::<TreeNode<K, T>>();
                    if !node_raw.is_null() {
                        let node = unsafe { NonNull::new_unchecked(node_raw) };
                        unsafe { slf.as_mut() }.right = Some(node);
                        unsafe {
                            node_raw.write(TreeNode {
                                parent: Some(slf),
                                left: None,
                                right: None,
                                color: Color::Red,
                                key,
                                val: value,
                            })
                        };
                        if unsafe { slf.as_ref() }.color == Color::Red {
                            println!("recoloring...!");
                            // we need fixing up the color properties
                            Self::recolor_2(node, root);
                        }
                    }
                }
            },
            Ordering::Equal => panic!("Tried to insert a key that is already present"),
            Ordering::Greater => {
                if let Some(node) = unsafe { slf.as_ref() }.left {
                    Self::insert(node, root, key, value);
                } else {
                    let node = unsafe { alloc(Layout::new::<TreeNode<K, T>>()) }.cast::<TreeNode<K, T>>();
                    if !node.is_null() {
                        unsafe {
                            node.write(TreeNode {
                                parent: Some(slf),
                                left: None,
                                right: None,
                                color: Color::Red,
                                key,
                                val: value,
                            })
                        };
                        let node = unsafe { NonNull::new_unchecked(node) };
                        unsafe { slf.as_mut() }.left = Some(node);
                        if unsafe { slf.as_ref() }.color == Color::Red {
                            println!("recoloring...!");
                            // we need fixing up the color properties
                            Self::recolor_2(node, root);
                        }
                    }
                }
            }
        }
    }

    fn recolor_2(node: NonNull<TreeNode<K, T>>, root: *mut Option<NonNull<TreeNode<K, T>>>) {
        let mut curr = node;
        let mut iter = 0;
        while let Some(mut parent) = unsafe { curr.as_ref() }.parent {
            println!("iter: {}", iter);
            iter += 1;
                if unsafe { parent.as_ref() }.color == Color::Black {
                    println!("got black parent!");
                    break;
                }
                if let Some(mut gp) = unsafe { parent.as_ref() }.parent {
                    let maybe_uncle = unsafe { gp.as_ref() }.get_sibling(parent);
                    if maybe_uncle.map(|uncle| unsafe { uncle.as_ref() }.color == Color::Red).unwrap_or(false) {
                        // recolor and if parent's parent is not root, recolor it and recheck
                        unsafe { parent.as_mut() }.color = Color::Black;
                        unsafe { maybe_uncle.unwrap().as_mut() }.color = Color::Black;
                        // check if gp isn't root
                        if unsafe { gp.as_ref() }.parent.is_none() {
                            break;
                        }
                        unsafe { gp.as_mut() }.color = Color::Red;
                        // try applying transformations on grand parent to fix it up as well.
                        curr = gp;
                        println!("red uncle!");
                    } else {
                        let parent_dir = unsafe { gp.as_ref() }.child_dir(parent);
                        let child_dir = unsafe { parent.as_ref() }.child_dir(curr);
                        let rotations = Self::map_rotations(parent_dir, child_dir);
                        if let Some(next_rot) = rotations.1 {
                            println!("rotate 1");
                            match rotations.0 {
                                Direction::Left => {
                                    unsafe { curr.as_mut() }.rot_left_par();
                                }
                                Direction::Right => {
                                    unsafe { curr.as_mut() }.rot_right_par();
                                }
                            }

                            // at this point curr is the parent of the previous parent because of the rotation
                            // so we now have to rotate curr
                            match next_rot {
                                Direction::Left => {
                                    unsafe { curr.as_mut() }.rot_left_par();
                                }
                                Direction::Right => {
                                    unsafe { curr.as_mut() }.rot_right_par();
                                }
                            }
                            // FIXME: is this recoloring correct? is there something missing?
                            unsafe { curr.as_mut() }.color = Color::Black;
                            unsafe { parent.as_mut() }.color = Color::Red;
                            unsafe { gp.as_mut() }.color = Color::Red;
                        } else {
                            println!("rotate 2");
                            match rotations.0 {
                                Direction::Left => {
                                    println!("rot left!");
                                    unsafe { parent.as_mut() }.rot_left_par();
                                }
                                Direction::Right => {
                                    println!("rot right!");
                                    unsafe { parent.as_mut() }.rot_right_par();
                                }
                            }
                            // FIXME: is this correct for recoloring?
                            unsafe { gp.as_mut() }.color = Color::Red;
                            unsafe { parent.as_mut() }.color = Color::Black;
                        }
                        // try checking the next node
                        if let Some(check) = unsafe { curr.as_ref() }.parent {
                            curr = check;
                        } else {
                            // we went through all things, so we should be fine now
                            break;
                        }
                    }
                    println!("went through!");
                } else {
                    panic!("weird state!");
                }
            }
        // check if node is root
        if unsafe { curr.as_ref() }.parent.is_none() {
            unsafe { curr.as_mut() }.color = Color::Black;
            unsafe { *root = Some(curr) };
        }
        }

    fn map_rotations(parent: Direction, child: Direction) -> (Direction, Option<Direction>) {
        // for cases like:
        // \
        //  P
        //   \
        //    C
        if parent == child {
            return (parent.rev(), None);
        }
        (child.rev(), Some(parent.rev()))
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

impl<K: Ord + Debug, T: Debug> Debug for TreeNode<K, T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        println!("color: {:?}", self.color);
        println!("key: {:?}", self.key);
        println!("val: {:?}", self.val);
        if let Some(left) = self.left {
            f.write_str(format!("left child of {:?}:\n", self.key).as_str())?;
            unsafe { left.as_ref() }.fmt(f)?;
        }
        if let Some(right) = self.right {
            f.write_str(format!("right child of {:?}:\n", self.key).as_str())?;
            unsafe { right.as_ref() }.fmt(f)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert() {
        let mut tree = RbTree::new();
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

    /*#[test]
    fn remove() {
        let mut tree = RbTree::new();
        for i in 0..20 {
            tree.insert(i, format!("test {}", i));
        }
        for i in 0..20 {
            assert_eq!(tree.remove(&i), Some(format!("test {}", i)));
            assert_eq!(tree.get(&i), None);
        }
    }*/

}

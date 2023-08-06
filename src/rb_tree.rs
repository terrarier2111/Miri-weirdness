pub struct RbTree<K: PartialOrd, T> {
    val: Option<TreeNode<K, T>>,
}

impl<K: PartialOrd, T> RbTree<K, T> {

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

}

pub struct TreeNode<K: PartialOrd, T> {
    left: Option<Box<TreeNode<K, T>>>,
    right: Option<Box<TreeNode<K, T>>>,
    color: bool, // FIXME: inline this into some pointer (either left or right)
    key: K,
    val: T,
}
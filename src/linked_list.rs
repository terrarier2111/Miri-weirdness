use core::mem;

#[repr(transparent)]
pub struct LinkedList<T>(LinkedListNode<T>);

impl<T> LinkedList<T> {

    #[inline]
    pub fn new(val: T) -> Self {
        LinkedListNode::new(val).to_list()
    }

    #[inline]
    pub fn value(&self) -> &T {
        &self.0.val
    }

    #[inline]
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.0.val
    }

    pub fn set_value(&mut self, val: T) -> T {
        mem::replace(&mut self.0.val, val)
    }

    pub fn next(&self) -> Option<&Box<LinkedListNode<T>>> {
        self.0.next.as_ref()
    }

    pub fn next_mut(&mut self) -> Option<&mut Box<LinkedListNode<T>>> {
        self.0.next.as_mut()
    }

    pub fn insert(&mut self, val: T) -> &mut Box<LinkedListNode<T>> {
        self.0.insert(val)
    }

}

impl<T: PartialEq> LinkedList<T> {

    pub fn contains(&self, val: &T) -> bool {
        self.0.contains(val)
    }

}

impl<T: Default> Default for LinkedList<T> {
    fn default() -> Self {
        LinkedList::new(T::default())
    }
}

pub struct LinkedListNode<T> {
    val: T,
    next: Option<Box<LinkedListNode<T>>>,
}

impl<T> LinkedListNode<T> {

    #[inline]
    pub fn new(val: T) -> Self {
        Self {
            val,
            next: None,
        }
    }

    #[inline(always)]
    pub fn to_list(self) -> LinkedList<T> {
        LinkedList(self)
    }

    #[inline]
    pub fn value(&self) -> &T {
        &self.val
    }

    #[inline]
    pub fn value_mut(&mut self) -> &mut T {
        &mut self.val
    }

    pub fn set_value(&mut self, val: T) -> T {
        mem::replace(&mut self.val, val)
    }

    pub fn next(&self) -> Option<&Box<LinkedListNode<T>>> {
        self.next.as_ref()
    }

    pub fn next_mut(&mut self) -> Option<&mut Box<LinkedListNode<T>>> {
        self.next.as_mut()
    }

    pub fn replace_next(&mut self, val: T) -> Option<Box<LinkedListNode<T>>> {
        self.next.replace(Box::new(LinkedListNode::new(val)))
    }

    pub fn replace_next_raw(&mut self, node: LinkedListNode<T>) -> Option<Box<LinkedListNode<T>>> {
        self.next.replace(Box::new(node))
    }

    pub fn insert(&mut self, val: T) -> &mut Box<Self> {
        if self.next.is_some() {
            return self.next.insert(Box::new(LinkedListNode::new(val)));
        }
        self.next = Some(Box::new(LinkedListNode::new(val)));
        self.next.as_mut().unwrap()
    }

    /*
    pub fn remove(&mut self) -> RemoveResult<T> {
        if self.next.is_some() {
            let next_ref = self.next.as_mut().unwrap().as_mut();
            mem::swap(self, next.as_mut()); // FIXME: Should we use mem::replace? or even smth else?
            // RemoveResult::Success(mem::replace(self, self.next.unwrap()))
            // mem::

        }
        RemoveResult::Failure
    }*/

}

/*
impl<T> LinkedListRemove<Box<LinkedListNode<T>>> for Box<LinkedListNode<T>> {
    fn remove(&mut self) -> RemoveResult<Box<LinkedListNode<T>>> {
        if self.next.is_some() {
            let next_ref = self.next.as_mut().unwrap();
            mem::swap(self, next_ref); // FIXME: Should we use mem::replace? or even smth else?
            next_ref.next = None;
            return RemoveResult::Success(next_ref.);
            // RemoveResult::Success(mem::replace(self, self.next.unwrap()))
            // mem::

        }
        RemoveResult::Failure
    }
}*/

/*
pub trait LinkedListRemove<T> {

    fn remove(&mut self) -> RemoveResult<T>;

}*/

impl<T: PartialEq> LinkedListNode<T> {

    pub fn contains(&self, val: &T) -> bool {
        if &self.val == val {
            return true;
        }
        self.next.as_ref().map_or(false, |next| next.contains(val))
    }

    /*
    pub fn remove_matching(&mut self, val: &T) -> RemoveResult<T> {
        if &self.val == val {

            return true;
        }
        self.next.as_mut().map_or(false, |next| next.remove_matching(val))
    }*/

}

pub enum RemoveResult<T> {
    Success(T),
    NotPresent,
    Failure,
}

use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicUsize, fence, Ordering};
use std::thread::yield_now;
use parking_lot_core::SpinWait;

pub struct UnfairMutex<T> {
    state: AtomicUsize,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for UnfairMutex<T> {}
unsafe impl<T: Sync> Sync for UnfairMutex<T> {}

const USED: usize = 1 << 0;

impl<T> UnfairMutex<T> {

    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicUsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    #[inline]
    pub fn lock(&self) -> Guard<'_, T> {
        let mut spin_wait = SpinWait::new();
        while self.state.compare_exchange_weak(0, USED, Ordering::Acquire, Ordering::Relaxed).is_err() {
            spin_wait.spin();
        }

        Guard(self)
    }

}

pub struct Guard<'a, T>(&'a UnfairMutex<T>);

impl<'a, T> Deref for Guard<'a, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.0.data.get() }
    }
}

impl<'a, T> DerefMut for Guard<'a, T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.0.data.get() }
    }
}

impl<'a, T> Drop for Guard<'a, T> {
    #[inline]
    fn drop(&mut self) {
        self.0.state.fetch_sub(USED, Ordering::Release);
    }
}

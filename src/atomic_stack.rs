use std::ptr::null_mut;
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct AtomicStack<T> {
    data: *mut T,
    capacity: AtomicUsize,
    top: AtomicUsize,
    bottom: AtomicUsize,
}

impl<T> AtomicStack<T> {

    #[inline]
    pub const fn new() -> Self {
        Self {
            data: null_mut(),
            capacity: AtomicUsize::new(0),
            top: AtomicUsize::new(0),
            bottom: AtomicUsize::new(0),
        }
    }

    pub fn push(&self, data: T) {
        let top = self.top.fetch_add(1, Ordering::Acquire);
        // FIXME: what does this non-atomic store synchronize with?
        *unsafe { &mut *self.data.add(top + 1) } = data;

        loop {
            while self.bottom.load(Ordering::Relaxed) != top {}

            if self.bottom.compare_exchange_weak(top, top + 1, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                break;
            }
        }
    }

}

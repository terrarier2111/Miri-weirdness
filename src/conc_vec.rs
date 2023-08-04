/*use std::alloc::{alloc, dealloc, Layout};
use std::ops::Deref;
use std::process::abort;
use std::{alloc, ptr};
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use crossbeam_utils::{Backoff, CachePadded};
use likely_stable::unlikely;
use swap_arc::{SwapArc, SwapArcAnyMeta, SwapArcOption};

pub struct ConcurrentVec<T> {
    alloc: SwapArcOption<AllocArena<T>>,
    guard: CachePadded<AtomicUsize>,
}

struct AllocArena<T> {
    data_alloc: SizedAlloc<T>,
    meta_alloc: SizedAlloc<AtomicU8>,
}

#[derive(Copy, Clone, PartialEq)]
#[repr(usize)]
enum State {
    Idle = 0,
    Push = 1,
    Pop = 2,
    Lock = 3,
}

impl State {

    fn to_raw(self) -> usize {
        (self as usize) << (usize::BITS as usize - 2)
    }

    fn from_raw(raw: usize) -> Self {
        (raw >> (usize::BITS as usize - 2)) as State
    }

}

const PUSH_OR_ITER_INC: usize = 1 << 0; // for 64 bit arch: 15 bits
const POP_INC: usize = 1 << ((usize::BITS as usize - 2) / 2 / 2); // for 64 bit arch: 15 bits | offset = (64 - 2) / 2 / 2 = 31 / 2 = 15 = 15
const LEN_INC: usize = 1 << (usize::BITS as usize / 2 - 2); // for 64 bit arch: 32 bits | offset = 64 / 2 - 2 = 30
const PUSH_OR_ITER_INC_BITS: usize = (POP_INC - PUSH_OR_ITER_INC) >> 2; // leave the two msb free (always 0), so we have a buffer
const POP_INC_BITS: usize = ((1 << (usize::BITS as usize - 2)) - POP_INC) >> 2; // leave the two msb free (always 0), so we have a buffer

const SCALE_FACTOR: usize = 2;
const INITIAL_CAP: usize = 8;

impl<T> ConcurrentVec<T> {
    pub fn new() -> Self {
        Self {
            alloc: SwapArcAnyMeta::new(None),
            guard: Default::default(),
        }
    }

    pub fn push(&self, val: T) {
        // inc push_or_iter counter
        let mut curr_guard = self.guard.fetch_add(PUSH_OR_ITER_INC, Ordering::Acquire);
        while curr_guard & PUSH_OR_ITER_FLAG == 0 {
            let mut backoff = Backoff::new();
            // wait until the POP_FLAG and LOCK_FLAG are unset
            while curr_guard & (POP_FLAG | LOCK_FLAG) != 0 {
                backoff.snooze();
                curr_guard = self.guard.load(Ordering::Acquire);
            }
            match self.guard.compare_exchange(curr_guard, (curr_guard & !(POP_FLAG | LOCK_FLAG)) | PUSH_OR_ITER_FLAG, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(val) => {
                    curr_guard = val;
                }
            }
        }
        let pop_far = self.pop_far.load(Ordering::Acquire);
        let push_far = self.push_far.fetch_add(1, Ordering::Acquire);
        let slot = push_far - pop_far;

        // check if we have hit the soft guard bit, if so, recover
        if unlikely(push_far == SOFT_GUARD_BIT) {
            // recover by decreasing the current counter
            self.pop_far.store(0, Ordering::Release);
            self.push_far.fetch_sub(pop_far, Ordering::Release);
        }

        // check if we have hit the hard guard bit, if so, abort.
        if unlikely(push_far >= HARD_GUARD_BIT) {
            // we can't recover safely anymore because the vec grew too quickly.
            abort();
        }

        if let Some(cap) = self.alloc.load().as_ref() {
            let size = cap.size;
            if size == slot {
                let old_data_alloc = cap.data_alloc.ptr;
                let old_meta_alloc = cap.meta_alloc.ptr;
                drop(cap);
                unsafe { resize(self, size, old_data_alloc, old_meta_alloc, slot); }

                #[cold]
                unsafe fn resize<T>(slf: &ConcurrentVec<T>, size: usize, old_data_alloc: *mut T, old_meta_alloc: *mut T, slot: usize) {
                    // wait until all previous writes finished
                    let mut backoff = Backoff::new();
                    while slf.len.load(Ordering::Acquire) != slot {
                        backoff.snooze();
                    }
                    let data_alloc = unsafe { alloc(Layout::array::<T>(size * SCALE_FACTOR).unwrap()) }.cast();
                    unsafe { ptr::copy_nonoverlapping(old_data_alloc, data_alloc, size); }

                    let meta_alloc = unsafe { alloc::alloc_zeroed(Layout::array::<AtomicU8>(size * SCALE_FACTOR).unwrap()) }.cast();
                    unsafe { ptr::copy_nonoverlapping(old_meta_alloc, data_alloc, size); }

                    slf.alloc.store(Some(Arc::new(AllocArena {
                        data_alloc: SizedAlloc::new(data_alloc, size * SCALE_FACTOR),
                        meta_alloc: SizedAlloc::new(meta_alloc, size * SCALE_FACTOR),
                    })));

                    let mut backoff = Backoff::new();
                    // wait for the resize to be performed
                    while slf.alloc.load().as_ref().as_ref().unwrap().data_alloc.size <= slot {
                        backoff.snooze();
                    }
                }
            } else if cap.size < slot {
                drop(cap);
                let mut backoff = Backoff::new();
                // wait for the resize to be performed
                while self.alloc.load().as_ref().as_ref().unwrap().data_alloc.size <= slot {
                    backoff.snooze();
                }
            }
        } else {
            if slot == 0 {
                let data_alloc = unsafe { alloc(Layout::array::<T>(INITIAL_CAP).unwrap()) }.cast();
                let meta_alloc = unsafe { alloc(Layout::array::<u8>(INITIAL_CAP).unwrap()) }.cast();

                self.alloc.store(Some(Arc::new(AllocArena {
                    data_alloc: SizedAlloc::new(data_alloc, INITIAL_CAP),
                    meta_alloc: SizedAlloc::new(meta_alloc, INITIAL_CAP),
                })));
            }
            let mut backoff = Backoff::new();
            // wait for the resize to be performed
            while self.alloc.load().as_ref().is_none() {
                backoff.snooze();
            }
        }
        loop {
            let cap = self.alloc.load();
            let cap = cap.as_ref().as_ref().unwrap();
            if cap.data_alloc.size <= slot {
                continue;
            }
            unsafe { cap.data_alloc.ptr.add(slot).write(val); }

            let mut backoff = Backoff::new();
            // we have to ensure that all slots up to len_new
            // are inhabited before we can increment the len
            while self.len.compare_exchange_weak(slot, slot + 1, Ordering::Release, Ordering::Relaxed).is_err() {
                backoff.snooze();
            }

            break;
        }

        self.dec_push_or_iter_cnt();
    }

    pub fn pop(&self) -> Option<T> {
        // dec pop counter
        let mut curr_guard = self.guard.fetch_add(POP_INC, Ordering::Acquire);
        while curr_guard & POP_FLAG == 0 {
            let mut backoff = Backoff::new();
            // wait until the PUSH_OR_ITER_FLAG and LOCK_FLAG are unset
            while curr_guard & (PUSH_OR_ITER_FLAG | LOCK_FLAG) != 0 {
                backoff.snooze();
                curr_guard = self.guard.load(Ordering::Acquire);
            }
            match self.guard.compare_exchange(curr_guard, curr_guard | POP_FLAG, Ordering::Acquire, Ordering::Acquire) {
                Ok(_) => break,
                Err(val) => {
                    curr_guard = val;
                }
            }
        }
        let push_far = self.push_far.load(Ordering::Acquire);
        let pop_far = self.pop_far.fetch_add(1, Ordering::AcqRel);

        if unlikely(pop_far >= push_far) {
            self.pop_far.fetch_sub(1, Ordering::Relaxed);
            self.dec_pop_cnt();
            return None;
        }

        // the idx of the last inhabited element (take the pushed counter and subtract the popped counter from it,
        // and subtract 1 from it in order to adjust for converting from count to index)
        let slot = push_far - pop_far - 1;

        // check if we have hit the hard guard bit, if so, abort.
        if unlikely(push_far >= HARD_GUARD_BIT) {
            // we can't recover safely anymore because the vec grew too quickly.
            abort();
        }

        let ret = if let Some(cap) = self.alloc.load().as_ref() {
            let val = unsafe { cap.data_alloc.ptr.add(slot).read() };
            unsafe { cap.meta_alloc.ptr.add(slot).as_ref().unwrap() }.store(0, Ordering::Release);
            self.len.fetch_sub(1, Ordering::Release);
            Some(val)
        } else {
            None
        };

        self.dec_pop_cnt();

        ret
    }

    pub fn remove(&self, idx: usize) -> Option<T> {
        let mut backoff = Backoff::new();
        while self.guard.compare_exchange_weak(0, LOCK_FLAG, Ordering::Acquire, Ordering::Relaxed).is_err() {
            backoff.snooze();
        }

        let ret = if let Some(alloc) = self.alloc.load().deref() {
            let val = unsafe { alloc.deref().ptr.add(idx).read() };
            let trailing = alloc.size - idx - 1;
            if trailing != 0 {
                unsafe { ptr::copy(alloc.deref().ptr.add(idx + 1), alloc.deref().ptr.add(idx), trailing) };
            }
            self.len.store(self.len() - 1, Ordering::Release);
            Some(val)
        } else {
            None
        };

        self.guard.fetch_and(!LOCK_FLAG, Ordering::Release);

        ret
    }

    pub fn insert(&self, idx: usize, val: T) {
        let mut backoff = Backoff::new();
        while self.guard.compare_exchange_weak(0, LOCK_FLAG, Ordering::Acquire, Ordering::Relaxed).is_err() {
            backoff.snooze();
        }

        let len = self.len.load(Ordering::Acquire);
        if len < idx {
            panic!("Index out of bounds ({}) while len is {}.", idx, len);
        }

        let req = len + 1;
        if let Some(cap) = self.alloc.load().as_ref() {
            let size = cap.size;
            if size < req {
                let old_data_alloc = cap.data_alloc.ptr;
                let old_meta_alloc = cap.meta_alloc.ptr;
                drop(cap);
                unsafe { resize(self, size, old_data_alloc, old_meta_alloc); }

                #[cold]
                unsafe fn resize<T>(slf: &ConcurrentVec<T>, size: usize, old_data_alloc: *mut T, old_meta_alloc: *mut T) {
                    let data_alloc = unsafe { alloc(Layout::array::<T>(size * SCALE_FACTOR).unwrap()) }.cast();
                    let meta_alloc = unsafe { alloc(Layout::array::<AtomicU8>(size * SCALE_FACTOR).unwrap()) }.cast();
                    unsafe { ptr::copy_nonoverlapping(old_data_alloc, data_alloc, size); }
                    unsafe { ptr::copy_nonoverlapping(old_meta_alloc, meta_alloc, size); }
                    slf.alloc.store(Some(Arc::new(AllocArena {
                        data_alloc: SizedAlloc::new(data_alloc, size * SCALE_FACTOR),
                        meta_alloc: SizedAlloc::new(meta_alloc, size * SCALE_FACTOR),
                    })));
                }
            }
        } else {
            let data_alloc = unsafe { alloc(Layout::array::<T>(INITIAL_CAP).unwrap()) }.cast();
            let meta_alloc = unsafe { alloc(Layout::array::<AtomicU8>(INITIAL_CAP).unwrap()) }.cast();
            self.alloc.store(Some(Arc::new(AllocArena {
                data_alloc: SizedAlloc::new(data_alloc, INITIAL_CAP),
                meta_alloc: SizedAlloc::new(meta_alloc, INITIAL_CAP),
            })));

            let mut backoff = Backoff::new();
            // wait for the resize to be performed
            while self.alloc.load().as_ref().is_none() {
                backoff.snooze();
            }
        }

        let alloc = self.alloc.load();
        let alloc = alloc.as_ref().as_ref().unwrap();
        let alloc = alloc.deref();
        let data_ptr = alloc.data_alloc.ptr;
        let meta_ptr = alloc.meta_alloc.ptr;
        let len = self.len();
        unsafe { ptr::copy(data_ptr.add(idx), data_ptr.add(idx).add(1), len - idx); }
        unsafe { data_ptr.add(idx).write(val); }
        unsafe { ptr::copy(meta_ptr.add(idx), meta_ptr.add(idx).add(1), len - idx); }
        unsafe { meta_ptr.add(idx).as_ref().unwrap() }.store(1, Ordering::Release);
        self.len.store(len + 1, Ordering::Release);

        self.guard.fetch_and(!LOCK_FLAG, Ordering::Release);
    }

    pub fn iter(&self) -> Iter<'_, T> {
        // inc push_or_iter counter
        let mut curr_guard = self.guard.fetch_add(PUSH_OR_ITER_INC, Ordering::Acquire);
        while curr_guard & PUSH_OR_ITER_FLAG == 0 {
            let mut backoff = Backoff::new();
            // wait until the POP_FLAG and REM_FLAG are unset
            while curr_guard & (POP_FLAG | LOCK_FLAG) != 0 {
                backoff.snooze();
                curr_guard = self.guard.load(Ordering::Acquire);
            }
            match self.guard.compare_exchange(curr_guard, (curr_guard & !(POP_FLAG | LOCK_FLAG)) | PUSH_OR_ITER_FLAG, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => break,
                Err(val) => {
                    curr_guard = val;
                }
            }
        }
        Iter {
            parent: self,
            idx: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len.load(Ordering::Acquire)
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn dec_push_or_iter_cnt(&self) {
        let mut guard_end = self.guard.fetch_sub(PUSH_OR_ITER_INC, Ordering::Release) - PUSH_OR_ITER_INC;
        // check if somebody else will unset the flag
        while guard_end & PUSH_OR_ITER_INC_BITS == 0 {
            // get rid of the flag if there are no more on-going pushes or iterations
            match self.guard.compare_exchange(guard_end, guard_end & !PUSH_OR_ITER_FLAG, Ordering::Release, Ordering::Relaxed) {
                Ok(_) => break,
                Err(err) => {
                    guard_end = err;
                }
            }
        }
    }

    fn dec_pop_cnt(&self) {
        let mut guard_end = self.guard.fetch_sub(POP_INC, Ordering::Release) - POP_INC;
        // check if somebody else will unset the flag
        while guard_end & POP_INC_BITS == 0 {
            // get rid of the flag if there are no more on-going pushes or iterations
            match self.guard.compare_exchange(guard_end, guard_end & !POP_FLAG, Ordering::Release, Ordering::Relaxed) {
                Ok(_) => break,
                Err(err) => {
                    guard_end = err;
                }
            }
        }
    }

}

impl<T> Drop for ConcurrentVec<T> {
    fn drop(&mut self) {
        if let Some(alloc) = self.alloc.load().as_ref() {
            alloc.set_partially_init(*self.len.get_mut());
        }
    }
}

struct SizedAlloc<T> {
    size: usize,
    ptr: *mut T,
    len: AtomicUsize,
}

impl<T> SizedAlloc<T> {

    #[inline]
    const fn new(ptr: *mut T, size: usize) -> Self {
        Self {
            size,
            ptr,
            len: AtomicUsize::new(0),
        }
    }

    fn set_partially_init(&self, len: usize) {
        if len > self.size {
            // this is not allowed
            abort();
        }
        self.len.store(len, Ordering::Release);
    }

}

impl<T> Drop for SizedAlloc<T> {
    fn drop(&mut self) {
        for x in 0..*self.len.get_mut() {
            unsafe { self.ptr.offset(x as isize).drop_in_place(); }
        }
        unsafe { dealloc(self.ptr.cast(), Layout::array::<T>(self.size).unwrap()) };
    }
}

unsafe impl<T> Send for SizedAlloc<T> {}
unsafe impl<T> Sync for SizedAlloc<T> {}

pub struct Iter<'a, T> {
    parent: &'a ConcurrentVec<T>,
    idx: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let len = self.parent.len.load(Ordering::Acquire);
        if len <= self.idx {
            return None;
        }

        let ret = unsafe { self.parent.alloc.load().as_ref().as_ref().unwrap().deref().ptr.add(self.idx) };

        self.idx += 1;

        Some(unsafe { ret.as_ref().unwrap() })
    }
}

impl<'a, T> Drop for Iter<'a, T> {
    fn drop(&mut self) {
        self.parent.dec_push_or_iter_cnt();
    }
}
*/
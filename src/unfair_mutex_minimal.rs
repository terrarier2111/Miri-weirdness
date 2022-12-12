use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicIsize, AtomicU16, AtomicU32, AtomicU8, AtomicUsize, fence, Ordering};
use std::thread::yield_now;
use cfg_if::cfg_if;
use parking_lot_core::{DEFAULT_PARK_TOKEN, DEFAULT_UNPARK_TOKEN, FilterOp, ParkResult, ParkToken, SpinWait, UnparkResult, UnparkToken};

pub struct UnfairMutex<T> {
    state: AtomicU8,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for UnfairMutex<T> {}
unsafe impl<T: Sync> Sync for UnfairMutex<T> {}

impl<T> UnfairMutex<T> {

    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicU8::new(0),
            data: UnsafeCell::new(data),
        }
    }

    #[inline]
    pub fn lock(&self) -> Guard<'_, T> {
        // println!("tid: {} ticket: {}", thread_id::get(), ticket);

        if self.state.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            self.lock_slow();
        }

        // fence(Ordering::Acquire);

        // println!("success!");

        Guard(self)
    }

    #[cold]
    fn lock_slow(&self) {
        let mut spin_wait = SpinWait::new();
        let mut state = self.state.load(Ordering::Relaxed);
        while self.state.compare_exchange_weak(0, 1, Ordering::Acquire, Ordering::Relaxed).is_err() {
            // yield_now();
            // let raw = self.state.load(Ordering::Acquire);
            // println!("ticket: {} wait raw: {} | {}", ticket, raw, (raw >> (usize::BITS / 2)));
            // spin_loop();


            // spin_wait.spin();
            spin_wait.spin();

            state = self.state.load(Ordering::Relaxed);
        }
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
        // let req_token = (self.0.state.fetch_add(SECOND_CNT_ONE, Ordering::Release) & !PARKED_BIT) >> (usize::BITS / 2);
        // set_first_half(&self.0.state, (get_curr_ticket(raw_req_token) + 1) as Half, Ordering::Release);
        // fence(Ordering::Acquire);
        self.0.state.store(0, Ordering::Release);
    }
}

/*
#[inline(never)]
#[no_mangle]
pub fn drop_cool_name<'a, T>(slf: &Guard<'a, T>) {
    // let req_token = (self.0.state.fetch_add(SECOND_CNT_ONE, Ordering::Release) & !PARKED_BIT) >> (usize::BITS / 2);
    let raw_req_token = slf.0.state.load(Ordering::Relaxed);
    set_first_half(&slf.0.state, (get_curr_ticket(raw_req_token) + 1) as Half, Ordering::Release);
    // fence(Ordering::Acquire);
}

#[inline(never)]
#[no_mangle]
pub fn lock_cool_name<T>(slf: &FairMutex<T>) -> Guard<'_, T> {
    let raw = slf.state.fetch_add(SECOND_CNT_ONE, Ordering::Acquire) & TICKET_MASK;
    let ticket = raw & TICKET_MASK;
    // println!("tid: {} ticket: {}", thread_id::get(), ticket);

    if ticket != (raw & CURR_TICKET_MASK) {
        slf.lock_slow(ticket);
    }

    // fence(Ordering::Acquire);

    // println!("success!");

    Guard(slf)
}

#[cold]
#[no_mangle]
#[inline(never)]
pub fn lock_slow_cool_name<T>(slf: &FairMutex<T>, ticket: usize) {
    let mut spin_wait = SpinWait::new();
    let mut state = slf.state.load(Ordering::Relaxed);
    while (curr_ticket_to_ticket(get_curr_ticket(state)))/*((state & !PARKED_BIT) >> (usize::BITS / 2))*/ != ticket {
        // yield_now();
        // let raw = self.state.load(Ordering::Acquire);
        // println!("ticket: {} wait raw: {} | {}", ticket, raw, (raw >> (usize::BITS / 2)));
        // spin_loop();


        // spin_wait.spin();
        spin_wait.spin();

        state = slf.state.load(Ordering::Relaxed);
    }
}

#[inline]
fn get_curr_ticket(state: usize) -> usize {
    state & CURR_TICKET_MASK
}

#[inline]
fn curr_ticket_to_ticket(curr_ticket: usize) -> usize {
    curr_ticket << (usize::BITS / 2)
}*/

const SECOND_CNT_ONE: usize = 1 << (usize::BITS / 2);

#[inline]
fn set_first_half(dst: &AtomicUsize, val: Half, ordering: Ordering) {
    unsafe { (&*(dst as *const AtomicUsize).cast::<AtomicHalf>()).store(val, ordering) };
}


cfg_if! {
    if #[cfg(target_pointer_width = "64")] {
        type Half = u32;
        type AtomicHalf = AtomicU32;
    } else if #[cfg(target_pointer_width = "32")] {
        type Half = u16;
        type AtomicHalf = AtomicU16;
    } else if #[cfg(target_pointer_width = "16")] {
        type Half = u8;
        type AtomicHalf = AtomicU8;
    }
}

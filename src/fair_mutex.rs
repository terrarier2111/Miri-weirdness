use std::cell::UnsafeCell;
use std::hint::spin_loop;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU8, AtomicUsize, fence, Ordering};
use std::thread::yield_now;
use cfg_if::cfg_if;
use parking_lot_core::{DEFAULT_PARK_TOKEN, DEFAULT_UNPARK_TOKEN, FilterOp, ParkResult, ParkToken, SpinWait, UnparkResult};

pub struct FairMutex<T> {
    state: AtomicUsize,
    data: UnsafeCell<T>,
}

unsafe impl<T: Send> Send for FairMutex<T> {}
unsafe impl<T: Sync> Sync for FairMutex<T> {}

const TICKET_MASK: usize = (1 << (usize::BITS / 2)) - 1;
const PARKED_BIT: usize = 1 << (usize::BITS - 1);

impl<T> FairMutex<T> {

    pub const fn new(data: T) -> Self {
        Self {
            state: AtomicUsize::new(0),
            data: UnsafeCell::new(data),
        }
    }

    #[inline]
    pub fn lock(&self) -> Guard<'_, T> {
        let ticket = self.state.fetch_add(1, Ordering::Acquire) & TICKET_MASK;

        let mut spin_wait = SpinWait::new();
        let mut state = self.state.load(Ordering::Relaxed);
        while ((state & !PARKED_BIT) >> (usize::BITS / 2)) != ticket {
            // yield_now();
            // let raw = self.state.load(Ordering::Acquire);
            // println!("ticket: {} wait raw: {} | {}", ticket, raw, (raw >> (usize::BITS / 2)));
            // spin_loop();


            // spin_wait.spin();

            // If there is no queue, try spinning a few times
            if state & PARKED_BIT == 0 && spin_wait.spin() {
                state = self.state.load(Ordering::Relaxed);
                continue;
            }


            // Park our thread until we are woken up by an unlock
            let addr = self as *const _ as usize;
            let validate = || {
                // self.state.load(Ordering::Relaxed) == LOCKED_BIT | PARKED_BIT;
                let state = self.state.load(Ordering::Relaxed);
                (state & PARKED_BIT) != 0 && ((state & !PARKED_BIT) >> (usize::BITS / 2)) != ticket
            };
            let before_sleep = || {};
            let timed_out = |_, was_last_thread| {/*
                // Clear the parked bit if we were the last parked thread
                if was_last_thread {
                    self.state.fetch_and(!PARKED_BIT, Ordering::Relaxed);
                }
            */};
            // SAFETY:
            //   * `addr` is an address we control.
            //   * `validate`/`timed_out` does not panic or call into any function of `parking_lot`.
            //   * `before_sleep` does not call `park`, nor does it panic.
            match unsafe {
                parking_lot_core::park(
                    addr,
                    validate,
                    before_sleep,
                    timed_out,
                    ParkToken(ticket),
                    None,
                )
            } {
                // We were unparked normally, try acquiring the lock again
                ParkResult::Unparked(_) => break,

                // The validation function failed, try locking again
                ParkResult::Invalid => (),

                // Timeout expired
                ParkResult::TimedOut => unreachable!(),
            }

            // Loop back and try locking again
            spin_wait.reset();
        }

        fence(Ordering::Acquire);

        // println!("success!");

        Guard(self)
    }

}

pub struct Guard<'a, T>(&'a FairMutex<T>);

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
        let req_token = (self.0.state.fetch_add(SECOND_CNT_ONE, Ordering::Release) & !PARKED_BIT) >> (usize::BITS / 2);

        // Unpark one thread and leave the parked bit set if there might
        // still be parked threads on this address.
        let addr = self as *const _ as usize;
        let callback = |result: UnparkResult| {
            // If we are using a fair unlock then we should keep the
            // mutex locked and hand it off to the unparked thread.
            if true || (result.unparked_threads != 0/* && (force_fair || result.be_fair)*/ && result.be_fair) {
                // Clear the parked bit if there are no more parked
                // threads.
                /*if !result.have_more_threads {
                    self.state.store(LOCKED_BIT, Ordering::Relaxed);
                }*/
                return DEFAULT_UNPARK_TOKEN/*TOKEN_HANDOFF*/;
            }

            // Clear the locked bit, and the parked bit as well if there
            // are no more parked threads.
            if result.have_more_threads {
                self.0.state.fetch_or(PARKED_BIT, Ordering::Release);
            } else {
                self.0.state.fetch_and(!PARKED_BIT, Ordering::Release);
            }
            DEFAULT_UNPARK_TOKEN/*TOKEN_NORMAL*/
        };
        // SAFETY:
        //   * `addr` is an address we control.
        //   * `callback` does not panic or call into any function of `parking_lot`.
        unsafe {
            // parking_lot_core::unpark_one(addr, callback);
            // parking_lot_core::unpark_all(addr, DEFAULT_UNPARK_TOKEN/*callback*/);
            let mut unparked = FilterOp::Skip;
            parking_lot_core::unpark_filter(addr, |token| {
                if token == ParkToken(req_token) {
                    unparked = FilterOp::Stop;
                    FilterOp::Unpark
                } else {
                    unparked
                }
            }, callback);
        }
    }
}

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

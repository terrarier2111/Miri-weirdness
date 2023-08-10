use std::alloc::{alloc, dealloc, Layout};
use std::marker::PhantomData;
use std::ptr::{NonNull, null_mut};
use std::sync::atomic::{AtomicPtr, AtomicUsize, Ordering};

const PTR_WIDTH: usize = usize::BITS as usize;

// every bit in the metadata indicates whether a certain bucket is present.
// we could also store counters instead of bitmaps but that would increase
// our metadata memory consumption by a factor of 64.
// storing counters would allow us to deallocate buckets to the global
// system allocator again and not only reuse them internally.
pub struct ConcVec<T> {
    buckets: [AtomicPtr<()>; PTR_WIDTH],
    bucket_meta: [AtomicPtr<()>; PTR_WIDTH], // FIXME: how can we dealloc this? should we store it inline in the other allocs instead?
    meta_allocated: AtomicUsize, // the Nth bit indicates whether the metadata for bucket N was already allocated
    _phantom_data: PhantomData<T>,
}

#[inline]
const fn most_sig_set_bit(val: usize) -> Option<u32> {
    let mut i = 0;
    let mut ret = None;
    while i < usize::BITS {
        if val & (1 << i) != 0 {
            ret = Some(i);
        }
        i += 1;
    }
    ret
}

#[inline]
const fn first_free_bit(val: HalfUsize) -> Option<HalfUsize> {
    let mut i = 0;
    while i < usize::BITS {
        if val & (1 << i) == 0 {
            return Some(1 << i);
        }
        i += 1;
    }
    None
}

impl<T> ConcVec<T> {

    const NULL_PTR: AtomicPtr<()> = AtomicPtr::new(null_mut());

    pub fn new() -> Self {
        Self {
            buckets: [Self::NULL_PTR; PTR_WIDTH],
            bucket_meta: [Self::NULL_PTR; PTR_WIDTH],
            meta_allocated: Default::default(),
            _phantom_data: Default::default(),
        }
    }

    pub fn push(&self, val: T) {
        let raw_meta = self.meta_allocated.load(Ordering::Acquire);
        let highest_meta = most_sig_set_bit(raw_meta);
        match highest_meta {
            None => {
                let mut meta_alloc = NonNull::new(unsafe { alloc(Layout::new::<AtomicUsize>()) }).unwrap().cast::<AtomicUsize>();
                *unsafe { meta_alloc.as_mut() }.get_mut() = 1;
                match self.bucket_meta[0].compare_exchange(null_mut(), meta_alloc.cast::<()>().as_ptr(), Ordering::Release, Ordering::Acquire) {
                    Ok(_) => {
                        let val_alloc = NonNull::new(unsafe { alloc(Layout::new::<T>()) }).unwrap().cast::<T>().as_ptr();
                        unsafe { val_alloc.write(val); }
                        self.buckets[0].store(val_alloc.cast::<()>(), Ordering::Release);
                        self.meta_allocated.fetch_or(1 << 0, Ordering::Release);
                    }
                    Err(new_alloc) => {
                        unsafe { dealloc(meta_alloc.as_ptr().cast::<u8>(), Layout::new::<AtomicUsize>()) };
                        let new_meta = unsafe { new_alloc.cast::<AtomicUsize>().as_ref().unwrap_unchecked() };
                        loop {
                            let new_meta_val = new_meta.load(Ordering::Acquire);
                            let first_free = first_free_bit((new_meta_val & HalfUsize::MAX as usize) as HalfUsize);
                            match first_free {
                                None => {
                                    self.push(val);
                                    return;
                                }
                                Some(bit) => {
                                    // try claiming our bit...
                                    if new_meta.fetch_or(bit as usize, Ordering::Release) & bit as usize == 0 { // FIXME: additionally we need a `removing` indicator
                                        // we succeeded in claiming our bit
                                    }
                                }
                            }
                        }
                    }
                }
            }
            Some(_) => {}
        }
    }

}

impl<T> Drop for ConcVec<T> {
    fn drop(&mut self) {
        for (i, bucket) in self.buckets.iter_mut().enumerate() {
            if bucket.get_mut().is_null() {
                break;
            }
            unsafe { dealloc(bucket.get_mut().cast::<u8>(), Layout::array::<T>(1 << i).unwrap_unchecked()); } // FIXME: drop the Ts
        }
        for (i, bucket) in self.bucket_meta.iter_mut().enumerate() {
            if bucket.get_mut().is_null() {
                break;
            }
            unsafe { dealloc(bucket.get_mut().cast::<u8>(), Layout::array::<AtomicUsize>(1 << i).unwrap_unchecked()); }
        }
    }
}

#[inline]
fn index(val: usize) -> (usize, usize, usize) {
    let bucket = usize::from(PTR_WIDTH) - ((val + 1).leading_zeros() as usize) - 1;
    let bucket_size = 1 << bucket;
    let index = val - (bucket_size - 1);
    (bucket, bucket_size, index)
}

#[derive(Copy, Clone, Debug)]
struct BucketMeta(usize);

impl BucketMeta {

    #[inline]
    fn present(self) -> HalfUsize {
        (self.0 & HalfUsize::MAX as usize) as HalfUsize
    }

    #[inline]
    fn removing(self) -> HalfUsize {
        (self.0 >> HalfUsize::BITS) as HalfUsize
    }

}

#[cfg(target_pointer_width = "64")]
type HalfUsize = u32;
#[cfg(target_pointer_width = "32")]
type HalfUsize = u16;
#[cfg(target_pointer_width = "16")]
type HalfUsize = u8;

use std::alloc::{dealloc, Layout};
use std::sync::atomic::{AtomicPtr, AtomicUsize};

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
}

impl<T> ConcVec<T> {

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

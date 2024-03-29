use std::alloc::{alloc, dealloc, Layout};
use std::{ptr, slice};
use std::mem::size_of;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicUsize, Ordering};
use crossbeam_utils::Backoff;

#[cfg(target_pointer_width = "128")]
type UHalfSize = u64;
#[cfg(target_pointer_width = "64")]
type UHalfSize = u32;
#[cfg(target_pointer_width = "32")]
type UHalfSize = u16;
#[cfg(target_pointer_width = "16")]
type UHalfSize = u8;
#[cfg(target_pointer_width = "8")]
type UHalfSize = !;

#[cfg(target_pointer_width = "128")]
type UHalfHalfSize = u32;
#[cfg(target_pointer_width = "64")]
type UHalfHalfSize = u16;
#[cfg(target_pointer_width = "32")]
type UHalfHalfSize = u8;
#[cfg(target_pointer_width = "16")]
type UHalfHalfSize = !;
#[cfg(target_pointer_width = "8")]
type UHalfHalfSize = !;

#[cfg(target_pointer_width = "128")]
type AtomicUHalfSize = AtomicU64;
#[cfg(target_pointer_width = "64")]
type AtomicUHalfSize = AtomicU32;
#[cfg(target_pointer_width = "32")]
type AtomicUHalfSize = AtomicU16;
#[cfg(target_pointer_width = "16")]
type AtomicUHalfSize = AtomicU8;
#[cfg(target_pointer_width = "8")]
type AtomicUHalfSize = !;

#[cfg(target_pointer_width = "128")]
type AtomicUHalfHalfSize = AtomicU32;
#[cfg(target_pointer_width = "64")]
type AtomicUHalfHalfSize = AtomicU16;
#[cfg(target_pointer_width = "32")]
type AtomicUHalfHalfSize = AtomicU8;
#[cfg(target_pointer_width = "16")]
type AtomicUHalfHalfSize = !;
#[cfg(target_pointer_width = "8")]
type AtomicUHalfHalfSize = !;

/// This is designed for a single remover thread and multiple pusher threads.
/// This data structure allows for a bunch of bytes of predefined capacity to be
/// stored in a ring buffer that allows for atomic addition and removal of said bytes.
/// The main disadvantage of this structure is that it is very limited in terms of
/// capacity. (for 64 bit arches it allows at most around 32k bytes) which can often
/// times be way to little as a buffer (for example for io buffers).
pub struct BBRing {
    buf: *mut u8,
    cap: usize,
    marker: AtomicUsize, // for 64 bit arch: head(32 bits) + len(16 bits) + finished_len(16 bits)
    remove_marker: AtomicUsize, // for 64 bit arch: rem_head(32 bits) + rem_len(16 bits) + finished_rem_len(16 bits)
}

impl BBRing {

    pub fn new(cap: usize) -> Self {
        if cap >= UHalfHalfSize::MAX as usize / 2 - 1 {
            panic!("Capacity is too large!");
        }
        let buf = unsafe { alloc(Layout::from_size_align_unchecked(cap, 1)) };
        if buf.is_null() {
            panic!("There was an error allocating the ring buf");
        }
        Self {
            buf,
            cap,
            marker: AtomicUsize::new(Marker::default().0),
            remove_marker: Default::default(),
        }
    }

    pub fn push(&self, data: &[u8]) -> bool {
        let marker = Marker::from_raw(self.marker.fetch_add(Marker::new(size_of::<UHalfSize>() + data.len(), 1, 0).into_raw(), Ordering::AcqRel));
        if (marker.head() - Marker::from_raw(self.remove_marker.load(Ordering::Acquire)).head()) + data.len() + size_of::<UHalfSize>() >= self.cap {
            self.marker.fetch_sub(Marker::new(size_of::<UHalfSize>() + data.len(), 1, 0).into_raw(), Ordering::AcqRel);
            return false;
        }
        // write length header
        unsafe { self.buf.add(marker.head()).cast::<UHalfSize>().write_unaligned(data.len() as UHalfSize) };
        // write payload
        unsafe { ptr::copy(data as *const [u8] as *const u8, self.buf.add(marker.head() + size_of::<UHalfSize>()), data.len()) };

        let backoff = Backoff::new();
        while Marker::from_raw(self.marker.load(Ordering::Acquire)).finished_len() != marker.len() {
            backoff.snooze();
        }
        self.marker.fetch_add(Marker::new(0, 0, 1).into_raw(), Ordering::AcqRel);
        true
    }

    pub fn pop_front(&self) -> Option<BufGuard<'_>> {
        let mut rem = Marker::from_raw(self.remove_marker.fetch_add(Marker::new(0, 1, 0).into_raw(), Ordering::AcqRel));
        let base = Marker::from_raw(self.marker.load(Ordering::Acquire));
        if rem.len() >= base.finished_len() {
            // we don't have anything we could pop anymore.
            self.remove_marker.fetch_sub(Marker::new(0, 1, 0).into_raw(), Ordering::AcqRel);
            return None;
        }

        if rem.finished_len() != rem.len() {
            let backoff = Backoff::new();
            let mut marker = Marker::from_raw(self.remove_marker.load(Ordering::Acquire));
            while rem.len() != marker.finished_len() {
                backoff.snooze();
                marker = Marker::from_raw(self.remove_marker.load(Ordering::Acquire));
            }
            rem = marker;
        }

        let len = unsafe { self.buf.add(rem.head() % self.cap).cast::<UHalfSize>().read_unaligned() };

        Some(BufGuard {
            parent: self,
            ptr: unsafe { self.buf.add(rem.head() % self.cap + size_of::<UHalfSize>()) },
            len: len as usize,
        })
    }

}

impl Drop for BBRing {
    fn drop(&mut self) {
        unsafe { dealloc(self.buf, Layout::from_size_align_unchecked(self.cap, 1)) };
    }
}

unsafe impl Send for BBRing {}
unsafe impl Sync for BBRing {}

pub struct BufGuard<'a> {
    parent: &'a BBRing,
    ptr: *mut u8,
    len: usize,
}

impl AsRef<[u8]> for BufGuard<'_> {
    fn as_ref(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.ptr.cast_const(), self.len) }
    }
}

impl AsMut<[u8]> for BufGuard<'_> {
    fn as_mut(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

impl Drop for BufGuard<'_> {
    fn drop(&mut self) {
        self.parent.remove_marker.fetch_add(Marker::new(self.len + size_of::<UHalfSize>(), 0, 1).into_raw(), Ordering::AcqRel);
    }
}

#[derive(Default)]
struct Marker(usize);

impl Marker {

    #[inline]
    fn new(head: usize, len: usize, finished_len: usize) -> Self {
        Self(head | (len << (usize::BITS / 2)) | (finished_len << (usize::BITS / 2 + usize::BITS / 2 / 2)))
    }

    #[inline]
    fn from_raw(raw: usize) -> Self {
        Self(raw)
    }

    #[inline]
    fn head(&self) -> usize {
        self.0 & (u32::MAX as usize)
    }

    #[inline]
    fn len(&self) -> usize {
        let offset = usize::BITS / 2;
        (self.0 & ((u16::MAX as usize) << offset)) >> offset
    }

    #[inline]
    fn finished_len(&self) -> usize {
        let offset = usize::BITS / 2 + usize::BITS / 2 / 2;
        (self.0 & ((u16::MAX as usize) << offset)) >> offset
    }

    #[inline]
    fn into_raw(self) -> usize {
        self.0
    }

}

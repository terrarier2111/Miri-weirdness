use std::cell::UnsafeCell;
use std::{mem, ptr};
use std::boxed::ThinBox;
use std::marker::PhantomData;
use std::mem::{align_of, ManuallyDrop, MaybeUninit, size_of, transmute};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::{addr_of, addr_of_mut, NonNull, null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};

// FIXME: add safety comments mentioning non-moving requirements (pinning)

pub fn create_thin_box<T>() -> impl FnMut(T) -> *mut T {
    |val| {
        let mut bx = ThinBox::new(val);
        let ret = bx.deref_mut() as *mut T;
        mem::forget(bx);
        ret
    }
}

pub struct SlimPtrMut<T> {
    ptr: *mut T,
}

impl<T> SlimPtrMut<T> {

    #[inline]
    pub fn new_with(val: T, mut create: impl FnMut(T) -> *mut T) -> Self {
        if !is_inlined::<T>() {
            Self {
                ptr: create(val),
            }
        } else {
            let mut ptr: *mut T = null_mut();
            let addr = addr_of_mut!(ptr);
            unsafe { addr.cast::<UnsafeCell<T>>().write(UnsafeCell::new(val)); }
            Self {
                ptr,
            }
        }
    }

    /*pub fn new_pinned(val: &mut Pin<T>) -> Self where T: Sized + Copy + Clone + Unpin {
        if !is_inlined::<T>() {
            Self {
                ptr: (val as *mut Pin<T>).cast::<T>(),
            }
        } else {
            let mut ptr: *mut T = null_mut();
            let addr = addr_of_mut!(ptr);
            unsafe { addr.cast::<UnsafeCell<T>>().write(UnsafeCell::new(transmute::<Pin<T>, T>(*val))); }
            Self {
                ptr,
            }
        }
    }*/

    /// Note: the returned pointer is only guaranteed to be valid as long as
    /// this instance of `SlimPtrMut` is alive.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        if !is_inlined::<T>() {
            self.ptr
        } else {
            unsafe { (&*addr_of!(self.ptr).cast::<UnsafeCell<T>>()) }.get()
        }
    }

    /// Note: the returned pointer is only guaranteed to be valid as long as
    /// this instance of `SlimPtrMut` is alive.
    #[inline]
    pub fn as_ptr_mut(&mut self) -> *mut T {
        if !is_inlined::<T>() {
            self.ptr
        } else {
            unsafe { (&*addr_of!(self.ptr).cast::<UnsafeCell<T>>()) }.get()
        }
    }

    #[inline]
    pub fn replace(&mut self, new: T) -> T {
        // SAFETY: This is safe because we know that we are the only
        // one with access to the data (no references outside this function exist at this time)
        unsafe { self.as_ptr_mut().replace(new) }
    }

    #[inline]
    pub fn into_inner(self) -> T {
        // SAFETY: This is safe because we know that we are the only
        // one with access to the data (no references outside this function exist at this time)
        unsafe { self.as_ptr().read() }
    }

    #[inline]
    pub fn token(&self) -> UnsafeToken<'_, T, ReadWriteAccess> {
        /*let ptr = if !is_inlined::<T>() {
            self.ptr
        } else {
            unsafe { &*addr_of!(self.ptr).cast::<UnsafeCell<T>>() }.get()
        };*/
        let ptr = self.as_ptr().cast_mut();
        UnsafeToken {
            ptr,
            _phantom_data: Default::default(),
        }
    }

}

impl<T> AsRef<T> for SlimPtrMut<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }
}

impl<T> AsMut<T> for SlimPtrMut<T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        unsafe { &mut *self.as_ptr_mut() }
    }
}

impl<T> Deref for SlimPtrMut<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> DerefMut for SlimPtrMut<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}

pub struct SlimPtr<T> {
    ptr: *mut T,
}

impl<T> SlimPtr<T> {

    #[inline]
    pub fn new_with(val: T, mut create: impl FnMut(T) -> *mut T) -> Self {
        if !is_inlined::<T>() {
            Self {
                ptr: create(val),
            }
        } else {
            let mut ptr: *mut T = null_mut();
            let addr = addr_of_mut!(ptr);
            unsafe { addr.cast::<T>().write(val); }
            Self {
                ptr,
            }
        }
    }

    /// Note: the returned pointer is only guaranteed to be valid as long as
    /// this instance of `SlimPtr` is alive.
    #[inline]
    pub fn as_ptr(&self) -> *const T {
        if !is_inlined::<T>() {
            self.ptr
        } else {
            addr_of!(self.ptr).cast::<T>()
        }
    }

    #[inline]
    pub fn into_inner(self) -> T {
        unsafe { self.as_ptr().read() }
    }

    #[inline]
    pub fn token(&self) -> UnsafeToken<'_, T, ReadAccess> {
        /*let ptr = if !is_inlined::<T>() {
            self.ptr
        } else {
            addr_of_mut!(self.ptr).cast::<T>()
        };*/
        let ptr = self.as_ptr().cast_mut();
        UnsafeToken {
            ptr,
            _phantom_data: Default::default(),
        }
    }

}

impl<T> AsRef<T> for SlimPtr<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }
}

impl<T> Deref for SlimPtr<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

#[inline]
pub const fn needs_cleanup<T>() -> bool {
    !is_inlined::<T>()
}

#[derive(Copy, Clone)]
pub struct UnsafeToken<'a, T, Access> {
    ptr: *mut T,
    _phantom_data: PhantomData<&'a Access>,
}

impl<'a, T, Access> UnsafeToken<'a, T, Access> {

    /// Reads the value from the pointer.
    ///
    /// Safety: The caller has to ensure that no mutable reference to the
    /// `SlimPtr` or `SlimPtrMut` of origin exists and that no double
    /// drop occurs or that a double drop/duplication of the instance of
    /// `T` isn't problematic.
    /// Additionally, see [`ptr::read`] for safety concerns and examples.
    #[inline]
    pub unsafe fn read(&self) -> T {
        self.ptr.read()
    }

}

impl<'a, T> UnsafeToken<'a, T, ReadWriteAccess> {

    /// Writes `val` to the pointer.
    ///
    /// Safety: The caller has to ensure that no reference to the
    /// `SlimPtr` or `SlimPtrMut` of origin exists and that the
    /// current instance of `T` stored at the location of the
    /// pointer gets dropped if necessary.
    /// Additionally, see [`ptr::write`] for safety concerns and examples.
    #[inline]
    pub unsafe fn write(&self, val: T) {
        self.ptr.write(val);
    }

    /// Replaces the current value with `new`.
    ///
    /// Safety: The caller has to ensure that no reference to the
    /// `SlimPtr` or `SlimPtrMut` of origin exists.
    /// Additionally, see [`ptr::replace`] for safety concerns and examples.
    #[inline]
    pub unsafe fn replace(&self, new: T) -> T {
        self.ptr.replace(new)
    }

}

/*
struct If<const B: bool>;

trait True {}

impl True for If<true> {}

impl<T> Unpin for SlimPtrMut<T> where If<{ !is_inlined::<T>() }>: True {}*/

struct ReadAccess;

struct ReadWriteAccess;

#[inline]
const fn is_inlined<T>() -> bool {
    !(size_of::<UnsafeCell<T>>() > size_of::<*mut T>() || align_of::<UnsafeCell<T>>() > align_of::<*mut T>())
}

pub struct AtomicInlinablePtr<T> {
    ptr: AtomicPtr<T>,
}

impl<T> AtomicInlinablePtr<T> {

    #[inline]
    pub fn new_with(val: T, mut create: impl FnMut(T) -> *mut T) -> Self {
        if !is_inlined::<T>() {
            Self {
                ptr: AtomicPtr::new(create(val)),
            }
        } else {
            let mut ptr: *mut T = null_mut();
            let addr = addr_of_mut!(ptr);
            unsafe { addr.cast::<T>().write(val); }
            Self {
                ptr: AtomicPtr::new(ptr),
            }
        }
    }

    /// Note: the returned pointer is only guaranteed to be valid as long as
    /// this instance of `SlimPtr` is alive.
    #[inline]
    pub fn as_ptr(&self, ordering: Ordering) -> *const T {
        if !is_inlined::<T>() {
            self.ptr.load(ordering)
        } else {
            addr_of!(self.ptr).cast::<AtomicPtr<T>>().load(ordering)
        }
    }

    #[inline]
    pub fn into_inner(self, ordering: Ordering) -> T {
        unsafe { self.as_ptr(ordering).read() }
    }

    #[inline]
    pub fn token(&self) -> UnsafeToken<'_, T, ReadAccess> {
        /*let ptr = if !is_inlined::<T>() {
            self.ptr
        } else {
            addr_of_mut!(self.ptr).cast::<T>()
        };*/
        let ptr = self.as_ptr().cast_mut();
        UnsafeToken {
            ptr,
            _phantom_data: Default::default(),
        }
    }

}

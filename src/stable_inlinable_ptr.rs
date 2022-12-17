use std::cell::UnsafeCell;
use std::{mem, ptr};
use std::boxed::ThinBox;
use std::marker::PhantomData;
use std::mem::{align_of, ManuallyDrop, MaybeUninit, size_of, transmute};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::{NonNull, null, null_mut};

// TODO: can we remove the `unchecked`s or are they required for the compiler to elide the bound-checks?

fn create_thin_box<T>() -> impl FnMut(T) -> NonNull<T> {
    |val| {
        let mut bx = ThinBox::new(val);
        let ret = NonNull::new(bx.deref_mut() as *mut T).unwrap();
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
            let mut ptr = null_mut();
            let val = Unaligned(val);
            unsafe { ((&mut ptr) as *mut *mut T).cast::<Unaligned<T>>().write(val); }
            Self {
                ptr,
            }
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        if !is_inlined::<T>() {
            self.ptr
        } else {
            (&unsafe { &*((&self.ptr) as *const *mut T).cast::<Unaligned<T>>() }.0) as *const T
        }
    }

    #[inline]
    pub fn as_ptr_mut(&mut self) -> *mut T {
        if !is_inlined::<T>() {
            self.ptr
        } else {
            (&mut unsafe { &mut *((&mut self.ptr) as *mut *mut T).cast::<Unaligned<T>>() }.0) as *mut T
        }
    }

    pub fn replace(&mut self, new: T) -> T {
        if !is_inlined::<T>() {
            unsafe { self.ptr.replace(new) }
        } else {
            unsafe { mem::replace(&mut (&mut *(((&mut self.ptr) as *mut *mut T).cast::<Unaligned<T>>())).0, new) }
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

/*
impl<T> Into<T> for SlimPtrMut<T> {
    #[inline]
    fn into(self) -> T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr().read() }
        } else {
            // let ret = unsafe { self.val[0].0 };
            let ret = unsafe { (self.val.get_unchecked(0) as *mut ManuallyDrop<T>).read() }.into();
            mem::forget(self);
            ret
        }
    }
}*/

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
    ptr: *const T,
}

impl<T> SlimPtr<T> {

    #[inline]
    pub fn new_with(val: T, mut create: impl FnMut(T) -> *const T) -> Self {
        if !is_inlined::<T>() {
            Self {
                ptr: create(val),
            }
        } else {
            let mut ptr = null_mut();
            let val = Unaligned(val);
            unsafe { (&mut ptr as *mut *mut T).cast::<Unaligned<T>>().write(val); }
            Self {
                ptr,
            }
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        if !is_inlined::<T>() {
            self.ptr
        } else {
            (&unsafe { &*((&self.ptr) as *const *const T).cast::<Unaligned<T>>() }.0) as *const T
        }
    }

}

impl<T> AsRef<T> for SlimPtr<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }
}

/*
impl<T> Into<T> for SlimPtr<T> {
    #[inline]
    fn into(self) -> T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr().read() }
        } else {
            // let ret = unsafe { self.val[0].0 };
            let ret = unsafe { (self.val.get_unchecked(0) as *mut ManuallyDrop<T>).read() }.into();
            mem::forget(self);
            ret
        }
    }
}*/

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
pub struct UnsafeToken<'a, T> {
    ptr: *const T,
    _phantom_data: PhantomData<&'a ()>,
}

/*
trait Lengths: Sized {
    const PTRS_LEN: usize = ptrs_len::<Self>();
    const VALS_LEN: usize = vals_len::<Self>();
}

impl<T> Lengths for T {}*/

#[inline]
const fn is_inlined<T>() -> bool {
    !(size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>())
}

const fn ptrs_len<T>() -> usize {
    if !is_inlined::<T>() {
        1
    } else {
        0
    }
}

const fn vals_len<T>() -> usize {
    if !is_inlined::<T>() {
        0
    } else {
        1
    }
}

#[repr(packed)]
struct Unaligned<T>(T);

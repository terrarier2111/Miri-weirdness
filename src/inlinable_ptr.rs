use std::cell::UnsafeCell;
use std::{mem, ptr};
use std::boxed::ThinBox;
use std::mem::{align_of, ManuallyDrop, MaybeUninit, size_of, transmute};
use std::ops::{Deref, DerefMut};
use std::pin::Pin;
use std::ptr::NonNull;

// TODO: can we remove the `unchecked`s or are they required for the compiler to elide the bound-checks?

fn create_thin_box<T>() -> impl FnMut(T) -> NonNull<T> {
    |val| {
        let bx = ThinBox::new(val);
        let ret = NonNull::new(ret.deref() as *mut T).unwrap();
        mem::forget(bx);
        ret
    }
}

// repr(C) won't impact perf negatively here as it only disallows field reordering optimizations
// which aren't a thing anyways in this struct with only one `real` (non-zero sized) field
#[repr(C)]
pub struct SlimPtrMut<T> {
    ptr: [NonNull<T>; const {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            1
        } else {
            0
        }
    }],
    val: [ManuallyDrop<Unaligned<T>>; const {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            0
        } else {
            1
        }
    }],
}

impl<T> SlimPtrMut<T> {

    #[inline]
    pub fn new_with(val: T, create: impl FnMut(T) -> NonNull<T>) -> Self {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            Self {
                ptr: [create(val)],
                val: [],
            }
        } else {
            Self {
                ptr: [],
                val: [ManuallyDrop::new(Unaligned(val))],
            }
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr() }
        } else {
            unsafe { self.val.get_unchecked(0) as *const T }
        }
    }

    #[inline]
    pub fn as_ptr_mut(&mut self) -> *mut T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr() }
        } else {
            unsafe { self.val.get_unchecked(0) as *mut T }
        }
    }

    #[inline]
    pub fn as_mut(&mut self) -> &mut T {
        unsafe { &mut *self.as_ptr_mut() }
    }

    pub fn replace(&mut self, new: T) -> T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr().replace(new) }
        } else {
            unsafe { mem::replace(self.val.get_unchecked_mut(0), new) }
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

// repr(C) won't impact perf negatively here as it only disallows field reordering optimizations
// which aren't a thing anyways in this struct with only one `real` (non-zero sized) field
#[repr(C)]
pub struct SlimPtr<T> {
    ptr: [NonNull<T>; const {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            1
        } else {
            0
        }
    }],
    val: [ManuallyDrop<Unaligned<T>>; const {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            0
        } else {
            1
        }
    }],
}

impl<T> SlimPtr<T> {

    #[inline]
    pub fn new_with(val: T, create: impl FnMut(T) -> NonNull<T>) -> Self {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            Self {
                ptr: [create(val)],
                val: [],
            }
        } else {
            Self {
                ptr: [],
                val: [ManuallyDrop::new(Unaligned(val))],
            }
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr() }
        } else {
            unsafe { self.val.get_unchecked(0) as *const T }
        }
    }

}

impl<T> AsRef<T> for SlimPtr<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }
}

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
}

impl<T> Deref for SlimPtr<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

#[inline]
pub fn needs_cleanup<T>() -> bool {
    size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>()
}

#[repr(packed)]
struct Unaligned<T>(T);

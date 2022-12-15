use std::cell::UnsafeCell;
use std::{mem, ptr};
use std::boxed::ThinBox;
use std::mem::{align_of, ManuallyDrop, MaybeUninit, size_of, transmute};
use std::pin::Pin;
use std::ptr::NonNull;

pub type SlimPtrMutDrop<T, const DROP: bool> = SlimPtrMutGeneric<T, |val| {
    ThinBox::new()
}, DROP>;

pub trait Creator {}

impl<I: FnMut(T) -> NonNull<T>, T> Creator for I {}

pub trait CleanUp {}

impl<I: FnMut(NonNull<T>) -> T, T> CleanUp for I {}

// repr(C) won't impact perf negatively here as it only disallows field reordering optimizations
// which aren't a thing anyways in this struct with only one `real` (non-zero sized) field
#[repr(C)]
pub struct SlimPtrMutGeneric<T, const Create: impl FnMut(T) -> NonNull<T>, const CleanUp: impl FnMut(NonNull<T>) -> T, const DROP: bool = false> {
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

impl<T, C: FnMut(T) -> NonNull<T>, CU: FnMut(NonNull<T>) -> T, const Create: C, const CleanUp: CU, const DROP: bool> SlimPtrMutGeneric<T, C, CU, Create, CleanUp> {

    #[inline]
    pub fn new(val: T) -> Self {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            Self {
                ptr: [Create(val)],
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
            // FIXME: can we remove the `unchecked` here?
            unsafe { self.ptr.get_unchecked(0).as_ptr() }
        } else {
            // FIXME: can we remove the `unchecked` here?
            unsafe { self.val.get_unchecked(0) as *const T }
        }
    }

    #[inline]
    pub fn as_ptr_mut(&mut self) -> *mut T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            // FIXME: can we remove the `unchecked` here?
            unsafe { self.ptr.get_unchecked(0).as_ptr() }
        } else {
            // FIXME: can we remove the `unchecked` here?
            unsafe { self.val.get_unchecked(0) as *mut T }
        }
    }

    #[inline]
    pub fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
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

    pub fn into_val(mut self) -> T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr().read() }
        } else {
            // let ret = unsafe { self.val[0].0 };
            let ret = unsafe { (self.val.get_unchecked(0) as *mut T).read() };
            mem::forget(self);
            ret
        }
    }

}

impl<T, C: FnMut(T) -> NonNull<T>, CU: FnMut(NonNull<T>) -> T, const Create: C, const CleanUp: CU, const DROP: bool> Drop for SlimPtrMutGeneric<T, C, CU, Create, CleanUp> {
    fn drop(&mut self) {
        if DROP {
            if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
                unsafe { CleanUp(*self.ptr.get_unchecked(0)) };
            } else {
                // FIXME: can needs_drop improve the perf here or is the compiler able to figure this out on its own?
                unsafe { (self.val.get_unchecked(0) as *mut T).read() };
            }
        }
    }
}

// repr(C) won't impact perf negatively here as it only disallows field reordering optimizations
// which aren't a thing anyways in this struct with only one `real` (non-zero sized) field
#[repr(C)]
pub struct SlimPtrGeneric<T, C: FnMut(T) -> NonNull<T>, CU: FnMut(NonNull<T>) -> T, const Create: C, const CleanUp: CU, const DROP: bool = false> {
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

impl<T, C: FnMut(T) -> NonNull<T>, CU: FnMut(NonNull<T>) -> T, const Create: C, const CleanUp: CU, const DROP: bool> SlimPtrGeneric<T, C, CU, Create, CleanUp> {

    #[inline]
    pub fn new(val: T) -> Self {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            Self {
                ptr: [Create(val)],
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
            // FIXME: can we remove the `unchecked` here?
            unsafe { self.ptr.get_unchecked(0).as_ptr() }
        } else {
            // FIXME: can we remove the `unchecked` here?
            unsafe { self.val.get_unchecked(0) as *const T }
        }
    }

    #[inline]
    pub fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }

    pub fn into_val(mut self) -> T {
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
            unsafe { self.ptr.get_unchecked(0).as_ptr().read() }
        } else {
            // let ret = unsafe { self.val[0].0 };
            let ret = unsafe { (self.val.get_unchecked(0) as *mut T).read() };
            mem::forget(self);
            ret
        }
    }

}

impl<T, C: FnMut(T) -> NonNull<T>, CU: FnMut(NonNull<T>) -> T, const Create: C, const CleanUp: CU, const DROP: bool> Drop for SlimPtrGeneric<T, C, CU, Create, CleanUp> {
    fn drop(&mut self) {
        if DROP {
            if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() {
                unsafe { CleanUp(*self.ptr.get_unchecked(0)) };
            } else {
                // FIXME: can needs_drop improve the perf here or is the compiler able to figure this out on its own?
                unsafe { (self.val.get_unchecked(0) as *const T).read() };
            }
        }
    }
}

#[repr(packed)]
struct Unaligned<T>(T);

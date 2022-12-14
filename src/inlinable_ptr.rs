use std::cell::UnsafeCell;
use std::mem;
use std::mem::{align_of, MaybeUninit, size_of, transmute};
use std::pin::Pin;

pub struct SlimPtrMut<T, Create: FnMut(T) -> *mut T, CleanUp: FnMut(*mut T) -> T> {
    ptr: UnsafeCell<MaybeUninit<*mut T>>,
}

impl<T, Create: FnMut(T) -> *mut T, CleanUp: FnMut(*mut T) -> T> SlimPtrMut<T, Create, CleanUp> {

    pub fn new(val: T) -> Self {
        let ptr = if size_of::<MaybeUninit<T>>() > size_of::<MaybeUninit<*mut T>>() || align_of::<MaybeUninit<T>>() > align_of::<MaybeUninit<*mut T>>() {
            MaybeUninit::new(Create(val))
        } else {
            let mut ret = MaybeUninit::uninit();
            if size_of::<MaybeUninit<T>>() > 0 {
                unsafe { ret.as_mut_ptr().cast::<T>().write(val); }
            }

            ret
        };
        Self {
            ptr: ptr.into(),
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.as_ptr_mut()
    }

    #[inline]
    pub fn as_ptr_mut(&self) -> *mut T {
        unsafe { (&*self.ptr.get()).assume_init_read() }
    }

    #[inline]
    pub fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }

    #[inline]
    pub fn as_mut(&self) -> &mut T {
        unsafe { &mut *self.as_ptr_mut() }
    }

    pub fn replace(&mut self, new: T) -> T {
        if size_of::<MaybeUninit<T>>() > size_of::<MaybeUninit<*mut T>>() || align_of::<MaybeUninit<T>>() > align_of::<MaybeUninit<*mut T>>() {
            let mut val = MaybeUninit::new(new);
            let old = unsafe { self.ptr.get().read().assume_init().read() };
            unsafe { self.ptr.get().replace(val); }
            old
        } else {
            // unsafe { self.ptr.get().cast::<MaybeUninit<T>>().read().assume_init() }
            unsafe { transmute::<*mut T, T>(self.ptr.into_inner().assume_init()) }
        }
    }

    pub fn into_val(self) -> T {
        if size_of::<MaybeUninit<T>>() > size_of::<MaybeUninit<*mut T>>() || align_of::<MaybeUninit<T>>() > align_of::<MaybeUninit<*mut T>>() {
            unsafe { self.ptr.into_inner().assume_init().read() }
        } else {
            // unsafe { self.ptr.get().cast::<MaybeUninit<T>>().read().assume_init() }
            unsafe { transmute::<*mut T, T>(self.ptr.into_inner().assume_init()) }
        }
    }

}

pub struct SlimPtr<T, Create: FnMut(T) -> *const T, CleanUp: FnMut(*const T) -> T> {
    ptr: MaybeUninit<*const T>,
}

impl<T, Create: FnMut(T) -> *const T, CleanUp: FnMut(*const T) -> T> SlimPtr<T, Create, CleanUp> {

    pub fn new(val: T) -> Self {
        let ptr = if size_of::<MaybeUninit<T>>() > size_of::<MaybeUninit<*const T>>() || align_of::<MaybeUninit<T>>() > align_of::<MaybeUninit<*const T>>() {
            MaybeUninit::new(Create(val))
        } else {
            let mut ret = MaybeUninit::uninit();
            if size_of::<MaybeUninit<T>>() > 0 {
                unsafe { ret.as_mut_ptr().cast::<T>().write(val); }
            }

            ret
        };
        Self {
            ptr,
        }
    }

    #[inline]
    pub fn as_ptr(&self) -> *const T {
        self.as_ptr_mut()
    }

    #[inline]
    pub fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }

    pub fn into_val(self) -> T {
        if size_of::<MaybeUninit<T>>() > size_of::<MaybeUninit<*const T>>() || align_of::<MaybeUninit<T>>() > align_of::<MaybeUninit<*const T>>() {
            unsafe { self.ptr.assume_init().read() }
        } else {
            unsafe { transmute::<*const T, T>(self.ptr.assume_init()) }
        }
    }

}

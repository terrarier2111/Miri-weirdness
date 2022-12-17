use std::cell::UnsafeCell;
use std::{mem, ptr};
use std::boxed::ThinBox;
use std::marker::PhantomData;
use std::mem::{align_of, ManuallyDrop, MaybeUninit, size_of, transmute};
use std::ops::{CoerceUnsized, Deref, DerefMut/*, DispatchFromDyn*/};
use std::pin::Pin;
use std::ptr::NonNull;

// TODO: can we remove the `unchecked`s or are they required for the compiler to elide the bound-checks?

fn create_thin_box<T>() -> impl FnMut(T) -> NonNull<T> {
    |val| {
        let bx = ThinBox::new(val);
        let ret = NonNull::new(bx.deref() as *mut T).unwrap();
        mem::forget(bx);
        ret
    }
}

fn get_vtable<T: ?Sized>(val: &T) {
    let meta = ptr::metadata(val as *const T);
    meta
}

/*
// repr(C) won't impact perf negatively here as it only disallows field reordering optimizations
// which aren't a thing anyways in this struct with only one `real` (non-zero sized) field
#[repr(C)]
pub struct SlimDynMut<T> {
    data: In,
}

impl<T> SlimDynMut<T> {

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

impl<T> AsRef<T> for SlimDynMut<T> {
    #[inline]
    fn as_ref(&self) -> &T {
        unsafe { &*self.as_ptr() }
    }
}

impl<T> AsMut<T> for SlimDynMut<T> {
    #[inline]
    fn as_mut(&mut self) -> &mut T {
        unsafe { &mut *self.as_ptr_mut() }
    }
}

impl<T> Into<T> for SlimDynMut<T> {
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

impl<T> Deref for SlimDynMut<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl<T> DerefMut for SlimDynMut<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut()
    }
}*/

const CAN_INLINE_DYN: bool = align_of::<NonNull<()>>() > 1;
const INLINE_MARKER: usize = 1 << 0;

#[inline]
const fn is_inlined(val: usize) -> bool {
    (val & INLINE_MARKER) != 0
}

// repr(C) won't impact perf negatively here as it only disallows field reordering optimizations
// which aren't a thing anyways in this struct with only one `real` (non-zero sized) field
#[repr(C)]
pub struct SlimDyn<Dyn: ?Sized> {
    data: MaybeUninit<*const ()>, // data
    vtable: NonNull<()>, // meta
    _phantom_data: PhantomData<Dyn>,
}

impl<Dyn: ?Sized> SlimDyn<Dyn> {

    #[inline]
    pub fn new_with<T: CoerceUnsized<Dyn>>(val: T, create: impl FnMut(T) -> NonNull<T>) -> Self {
        if !is_trait::<Dyn>() {
            panic!("The `Dyn` type of `SlimDyn` has to be a trait!");
        }
        let meta = ptr::metadata(&val as *const T);
        if size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>() || !CAN_INLINE_DYN {
            let val = create(val);
            Self {
                data: MaybeUninit::new(val.as_ptr()),
                vtable: meta,
            }
        } else {
            let mut data = MaybeUninit::uninit();
            unsafe { data.as_mut_ptr().cast::<T>().write(val); }
            Self {
                data,
                vtable: meta,
            }
        }
    }

}

impl<Dyn: ?Sized> AsRef<Dyn> for SlimDyn<Dyn> {
    #[inline]
    fn as_ref(&self) -> &Dyn {
        if !CAN_INLINE_DYN || !is_inlined(self.vtable.as_ptr() as usize) {
            unsafe { ptr::from_raw_parts(self.data.assume_init_read(), self.vtable.as_ptr()) }
        } else {
            // unsafe { self.val.get_unchecked(0) as *const T }
            unsafe { ptr::from_raw_parts(self.data.as_ptr().cast::<()>(), self.vtable.as_ptr()) }
        }
    }
}

/*
impl<T> Into<T> for SlimDyn<T> {
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

impl<T> Deref for SlimDyn<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

#[inline]
pub const fn needs_cleanup<T>() -> bool {
    size_of::<T>() > size_of::<NonNull<T>>() || align_of::<T>() > align_of::<NonNull<T>>()
}

#[repr(packed)]
struct Unaligned<T>(T);

#[inline]
const fn is_trait<T: ?Sized>() -> bool {
    // check if we have a fat pointer here
    size_of::<*mut T>() != size_of::<*mut usize>()
}

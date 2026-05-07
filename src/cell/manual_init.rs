use std::mem::MaybeUninit;

#[repr(transparent)]
pub struct ManualInit<T>(MaybeUninit<T>);

impl<T> ManualInit<T> {
    #[inline(always)]
    pub const fn new(t: T) -> Self {
        Self(MaybeUninit::new(t))
    }

    #[inline(always)]
    pub fn write(&mut self, t: T) {
        self.0.write(t);
    }

    #[inline(always)]
    #[must_use]
    pub unsafe fn as_ref(&self) -> &T {
        unsafe { self.0.assume_init_ref() }
    }

    #[inline(always)]
    #[must_use]
    pub unsafe fn as_mut(&mut self) -> &mut T {
        unsafe { self.0.assume_init_mut() }
    }

    #[inline(always)]
    pub unsafe fn drop_in_place(&mut self) {
        unsafe { self.0.assume_init_drop() }
    }
}

impl<T: Default> Default for ManualInit<T> {
    #[inline(always)]
    fn default() -> Self {
        Self::new(T::default())
    }
}

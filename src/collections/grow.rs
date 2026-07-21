use std::mem;
use std::ops::{Deref, DerefMut};

pub(super) struct BoxSliceGrowth<'a, T> {
    target: &'a mut Box<[T]>,
    values: Vec<T>,
}

impl<'a, T> BoxSliceGrowth<'a, T> {
    pub(super) fn take(target: &'a mut Box<[T]>) -> Self {
        let values = mem::take(target).into_vec();
        Self { target, values }
    }
}

impl<T> Deref for BoxSliceGrowth<'_, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<T> DerefMut for BoxSliceGrowth<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

impl<T> Drop for BoxSliceGrowth<'_, T> {
    fn drop(&mut self) {
        *self.target = mem::take(&mut self.values).into_boxed_slice();
    }
}

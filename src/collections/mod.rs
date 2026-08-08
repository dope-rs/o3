pub mod arena;
pub mod batch;
pub mod fixed;
pub mod heap;
pub mod intrusive;
pub mod pinned;
pub mod queue;
pub mod slab;

use std::{mem, ops};

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

impl<T> ops::Deref for BoxSliceGrowth<'_, T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl<T> ops::DerefMut for BoxSliceGrowth<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.values
    }
}

impl<T> Drop for BoxSliceGrowth<'_, T> {
    fn drop(&mut self) {
        *self.target = mem::take(&mut self.values).into_boxed_slice();
    }
}

struct ClearGuard<'a, T: ?Sized> {
    value: &'a mut T,
    clear: fn(&mut T),
    armed: bool,
}

impl<'a, T: ?Sized> ClearGuard<'a, T> {
    fn run(value: &'a mut T, clear: fn(&mut T)) {
        let mut guard = Self {
            value,
            clear,
            armed: true,
        };
        (guard.clear)(guard.value);
        guard.armed = false;
    }
}

impl<T: ?Sized> Drop for ClearGuard<'_, T> {
    fn drop(&mut self) {
        if self.armed {
            (self.clear)(self.value);
        }
    }
}

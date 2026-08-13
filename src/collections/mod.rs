pub mod batch;
pub mod completion;
pub mod fixed;
pub mod heap;
pub mod intrusive;
pub mod pinned;
pub mod queue;
mod sealed;
pub mod slab;

use std::{mem, ops};

pub use sealed::{AllocationError, BoxExt, BoxSliceExt, VecExt};
pub(crate) use sealed::{BoxUninitExt, ClearGuard};

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

//! Private linear lease shared by resident completion registries.

use std::{cell::Cell, marker::PhantomData};

use crate::depot::queue::resident::{Key, raw::View};

pub trait Target {
    type Parked;

    fn detached_grant<T>(_view: View<'_, T>, _index: usize) -> Option<Self::Parked> {
        None
    }

    fn retire(&self, key: Key);
}

/// Two-word ownership of one slot in an exactly borrowed target.
/// The invariant reference retains the original registration lifetime and !Sync.
#[repr(transparent)]
pub struct Lease<'target, T: Target + ?Sized> {
    active: Option<(&'target T, Key)>,
    target: PhantomData<Cell<&'target T>>,
}

impl<'target, T: Target + ?Sized> Lease<'target, T> {
    pub const fn new() -> Self {
        Self {
            active: None,
            target: PhantomData,
        }
    }

    pub(super) fn active(&mut self) -> Option<(&'target T, &mut Key)> {
        let (target, key) = self.active.as_mut()?;
        Some((*target, key))
    }

    pub(super) fn set(&mut self, target: &'target T, key: Key) {
        self.active = Some((target, key));
    }

    pub(super) fn detach(&mut self) {
        self.active = None;
    }

    pub(super) fn is_bound(&self) -> bool {
        self.active.is_some()
    }
}

impl<T: Target + ?Sized> Default for Lease<'_, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Target + ?Sized> Drop for Lease<'_, T> {
    fn drop(&mut self) {
        if let Some((target, key)) = self.active {
            target.retire(key);
        }
    }
}

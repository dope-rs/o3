//! Private linear lease shared by resident completion registries.

use std::{cell::Cell, marker::PhantomData};

use crate::mem::{self, Return, lease::Open};

pub(super) trait Target {
    type Key: Copy;

    fn retire(&self, key: Self::Key);
}

struct Bound<'target, T: Target + ?Sized>(&'target T);

impl<T: Target + ?Sized> Return for Bound<'_, T> {
    type Item = T::Key;

    fn return_item(self, key: Self::Item) {
        self.0.retire(key);
    }
}

/// Two-word ownership of one slot in an exactly borrowed target.
/// The invariant reference retains the original registration lifetime and !Sync.
#[repr(transparent)]
pub(super) struct Lease<'target, T: Target + ?Sized> {
    active: Option<mem::Lease<Bound<'target, T>, Open>>,
    target: PhantomData<Cell<&'target T>>,
}

impl<'target, T: Target + ?Sized> Lease<'target, T> {
    pub(super) const fn new() -> Self {
        Self {
            active: None,
            target: PhantomData,
        }
    }

    pub(super) fn active(&mut self) -> Option<(&'target T, &mut T::Key)> {
        self.active.as_mut().map(|lease| {
            let (target, key) = lease.parts_mut();
            (target.0, key)
        })
    }

    pub(super) fn set(&mut self, target: &'target T, key: T::Key) {
        self.active = Some(mem::Lease::open(Bound(target), key));
    }

    pub(super) fn detach(&mut self) {
        if let Some(lease) = self.active.take() {
            lease.into_parts();
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.active.is_some()
    }
}

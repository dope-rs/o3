//! Single-value ownership in exclusively borrowed, uninitialized storage.

use std::mem::MaybeUninit;

use crate::{base::marker::ThreadBound, mem};

mod occupied;
pub use occupied::Occupied;

/// Owns an optional value and its lifecycle metadata.
#[derive(Debug)]
#[repr(C)]
pub struct Storage<T, M = ()> {
    pub(in crate::mem) value: Option<T>,
    metadata: M,
    bound: ThreadBound,
}

impl<T, M> Storage<T, M> {
    pub const fn new(metadata: M) -> Self {
        Self {
            value: None,
            metadata,
            bound: ThreadBound::NEW,
        }
    }

    pub const fn filled(value: T, metadata: M) -> Self {
        Self {
            value: Some(value),
            metadata,
            bound: ThreadBound::NEW,
        }
    }

    pub const fn metadata(&self) -> &M {
        &self.metadata
    }

    pub fn metadata_mut(&mut self) -> &mut M {
        &mut self.metadata
    }

    pub const fn get(&self) -> Option<&T> {
        self.value.as_ref()
    }

    pub fn get_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }

    pub(crate) fn replace(&mut self, value: T) -> Option<T> {
        self.value.replace(value)
    }

    pub fn occupied(&mut self) -> Option<mem::raw::Occupied<'_, T>> {
        self.value.as_ref()?;
        // SAFETY: this exclusive cell loan retains the initialization just checked.
        Some(unsafe { mem::raw::occupy(self) })
    }

    pub fn vacant(&mut self) -> Option<mem::raw::Vacant<'_, T>> {
        if self.value.is_some() {
            return None;
        }
        // SAFETY: the exclusive loan retains the vacancy just checked.
        Some(unsafe { mem::raw::vacancy(self) })
    }
}

/// Exclusive admission to initialize one value without inspecting old bytes.
pub type Vacant<'a, T> = Occupied<'a, MaybeUninit<T>>;

impl<'a, T> Vacant<'a, T> {
    /// Borrows storage that does not own a value's destruction obligation.
    pub fn new(value: &'a mut MaybeUninit<T>) -> Self {
        Self {
            value,
            bound: ThreadBound::NEW,
        }
    }

    /// # Safety
    /// This slot contains one valid, exclusively owned T with no prior pinning obligation.
    pub unsafe fn assume_init(self) -> Occupied<'a, T> {
        // SAFETY: both transparent phases retain the same exclusive pointer and lifetime.
        // T is initialized; moving this owner transfers its sole destruction obligation.
        unsafe { std::mem::transmute(self) }
    }

    /// Transfers the exact initialized value and its destruction to one owner.
    pub fn insert(self, value: T) -> Occupied<'a, T> {
        self.value.write(value);
        // SAFETY: write initialized this exact exclusively borrowed slot.
        unsafe { self.assume_init() }
    }
}

const _: () = {
    assert!(size_of::<Occupied<'_, ()>>() == size_of::<usize>());
    assert!(size_of::<Option<Occupied<'_, ()>>>() == size_of::<usize>());
};

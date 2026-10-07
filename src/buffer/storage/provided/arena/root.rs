use std::{cell, num};

use crate::buffer::storage::{
    provided,
    provided::{
        arena::{Active, Returned},
        fixed,
    },
};

/// Borrowed access to the exact active header and slot layout.
#[repr(transparent)]
pub struct Root<'d, T, C = (), L = cell::Cell<u32>> {
    pub(super) header: &'d Active<'d, T, C, L>,
}

impl<'d, T, C, L> Root<'d, T, C, L> {
    pub const fn context(&self) -> &'d C {
        &self.header.context
    }
    pub fn capacity(&self) -> num::NonZero<u32> {
        self.header.domain.state.capacity()
    }
    pub fn pop_returned(&self) -> Option<T> {
        self.header.domain.pop_returned().map(Returned::take)
    }
    pub fn has_returned(&self) -> bool {
        self.header.domain.state.has_returned()
    }
}

impl<T, C, L> Clone for Root<'_, T, C, L> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, C, L> Copy for Root<'_, T, C, L> {}

impl<'d, T, C> Root<'d, T, C> {
    pub fn vacant(&self) -> Option<provided::Vacant<'d, T>> {
        self.header.domain.next_free(())
    }
}

impl<'d, T, C> Root<'d, T, C, ()> {
    pub fn try_vacant(&self) -> Result<provided::Vacant<'d, T, (), &'d C>, fixed::Full> {
        self.header
            .domain
            .next_free(self.context())
            .ok_or(fixed::Full {
                capacity: self.capacity(),
            })
    }
}

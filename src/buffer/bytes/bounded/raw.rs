use std::ops;

use crate::{
    base::num::bounded::U32,
    buffer::{
        self,
        bytes::{self, bounded},
    },
};

/// An unchecked value awaiting its caller-owned extent proof.
#[repr(transparent)]
pub struct Input<T, const MAX: usize = { bounded::MAX }>(T);

impl<T, const MAX: usize> Input<T, MAX> {
    pub(in crate::buffer) fn into_inner(self) -> T {
        self.0
    }

    /// # Safety
    /// `value` must expose at most `MAX` bytes.
    /// Shared access must preserve the admitted extent.
    pub const unsafe fn new(value: T) -> Self {
        Self(value)
    }
}

impl<'a, const MAX: usize> Input<&'a [u8], MAX> {
    pub(in crate::buffer) const fn into_slice(self) -> &'a [u8] {
        self.0
    }
}

impl<S: ?Sized> Input<buffer::View<'_, S>> {
    /// # Safety
    /// `range` must be contained in this view's unchanged initialized extent.
    pub unsafe fn select(mut self, range: ops::Range<usize>) -> Self {
        // SAFETY: the caller proves this subrange of the retained source borrow.
        self.0.bytes = unsafe { self.0.bytes.get_unchecked(range) };
        self
    }
}

impl<T: AsRef<[u8]>> Input<T> {
    pub(in crate::buffer) fn count(value: &bounded::Value<T>) -> U32<0, { bounded::MAX as u32 }> {
        use crate::base::num::bounded;

        let len = value.as_ref().len();
        // SAFETY: admission proves this stable extent fits the signed byte count
        // domain, so conversion preserves the exact u32 value.
        unsafe { bounded::raw::Proof::assume(len as u32).retain() }
    }
}

impl Input<&[u8]> {
    pub(in crate::buffer) const fn prefix<const MAX: usize, const END: u32>(
        prefix: &bytes::Prefix<'_, MAX>,
    ) -> U32<0, END> {
        use crate::base::num::bounded;

        const { assert!(MAX <= END as usize) };
        let len = prefix.as_slice().len();
        // SAFETY: the immutable prefix retains len <= MAX, and the const bound
        // proves MAX <= END, so conversion preserves the exact u32 value.
        unsafe { bounded::raw::Proof::assume(len as u32).retain() }
    }
}

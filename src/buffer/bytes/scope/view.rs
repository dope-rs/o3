use std::{marker, num, ops};

use crate::buffer::{bytes::scope, is_within, storage::range, write::consume};

/// A real source borrow carrying its immutable owner's identity.
#[derive(Clone, Copy)]
pub struct View<'a, 'id> {
    pub(in crate::buffer::bytes::scope) bytes: &'a [u8],
    pub(in crate::buffer::bytes::scope) brand: marker::PhantomData<*mut &'id ()>,
}

impl<'a, 'id> View<'a, 'id> {
    pub const fn as_slice(&self) -> &'a [u8] {
        self.bytes
    }

    /// Projects an admitted selection from this same immutable source identity.
    pub fn get<P: scope::raw::Select<'id>>(self, proof: P) -> (&'a [u8], scope::Range<'id>) {
        let bounds = proof.bounds();
        // SAFETY: the selection retains this identity's immutable original
        // extent and its admitted range throughout this operation.
        let bytes = unsafe { self.bytes.get_unchecked(bounds.clone()) };
        (bytes, scope::Range::new(self, bounds))
    }

    pub fn try_get(self, bounds: ops::Range<usize>) -> Result<scope::Range<'id>, range::Error> {
        if !is_within(&bounds, self.bytes.len()) {
            return Err(range::Error::new(bounds, self.bytes.len()));
        }
        Ok(scope::Range::new(self, bounds))
    }

    pub fn try_rsplit<const N: usize>(
        self,
    ) -> Result<(&'a [u8; N], scope::Range<'id>), consume::Error> {
        let Some((prefix, tail)) = self.bytes.split_last_chunk::<N>() else {
            return Err(consume::Error::new(
                num::NonZero::<usize>::MIN.saturating_add(N.saturating_sub(1)),
                self.bytes.len(),
            ));
        };
        Ok((tail, scope::Range::new(self, 0..prefix.len())))
    }
}

impl<'id> scope::Source for View<'_, 'id> {
    type Selection = scope::Range<'id>;

    fn select(self, bytes: &[u8]) -> Option<Self::Selection> {
        Some(scope::Range::new(
            self,
            self.bytes.subslice_range(bytes)?.into(),
        ))
    }
}

const _: () = assert!(size_of::<View<'static, 'static>>() == 2 * size_of::<usize>());

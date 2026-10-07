use std::{marker, num::NonZero};

use crate::{
    buffer,
    buffer::{segments, storage},
    mem::rc,
};
/// A strict prefix bound retaining its exclusive owner until consumption.
pub(in crate::buffer) struct Tail<'a, T, N = usize> {
    source: &'a mut T,
    amount: N,
}

impl<'a, T: buffer::raw::Segment> Tail<'a, T> {
    pub(in crate::buffer) fn new(source: &'a mut T, amount: usize) -> Option<Self> {
        if amount >= source.as_ref().len() {
            return None;
        }
        Some(Self { source, amount })
    }

    /// Retains this prefix before committing any change to the original owner.
    pub(in crate::buffer) fn try_commit(self) -> Result<T, rc::Error> {
        let prefix = self.view().try_retain()?;
        self.commit();
        Ok(prefix)
    }
}

impl<'a, T: buffer::raw::Segment> Tail<'a, T, NonZero<usize>> {
    pub(in crate::buffer) fn positive(
        source: &'a mut segments::Item<T>,
        amount: NonZero<usize>,
    ) -> Option<Self> {
        if amount >= source.len() {
            return None;
        }
        Some(Self {
            source: &mut source.value,
            amount,
        })
    }
}

impl<T: buffer::raw::Segment, N: marker::Copy + Into<usize>> Tail<'_, T, N> {
    pub(in crate::buffer) fn view(&self) -> buffer::View<'_, T> {
        // SAFETY: admission retains this exact owner and its strict prefix bound.
        let bytes = unsafe { self.source.as_ref().get_unchecked(..self.amount.into()) };
        buffer::View {
            source: self.source,
            bytes,
        }
    }

    pub(in crate::buffer) fn commit(self) {
        let amount = self.amount;
        // SAFETY: this strict prefix retains the original exclusive owner loan.
        let extent = unsafe { self.source.extent() };
        // SAFETY: admission leaves a nonempty suffix in this same backing.
        *extent = unsafe { extent.restrict(amount.into()..extent.len()) };
    }
}

impl<T, N> AsRef<T> for Tail<'_, T, N> {
    fn as_ref(&self) -> &T {
        self.source
    }
}

const _: () = {
    assert!(size_of::<Tail<'static, storage::Shared>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Tail<'static, storage::Shared>>>() == 2 * size_of::<usize>());
    assert!(size_of::<Tail<'static, storage::Shared, NonZero<usize>>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Option<Tail<'static, storage::Shared, NonZero<usize>>>>()
            == 2 * size_of::<usize>()
    );
};

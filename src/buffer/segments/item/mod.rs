use crate::buffer::storage::regions;
pub(in crate::buffer::segments) mod seal;

use std::{num::NonZero, ops};

use crate::{
    buffer,
    buffer::{resident, segments, storage},
    mem::rc,
};

/// A retained segment owner proved to expose at least one byte.
#[derive(Debug)]
#[repr(transparent)]
pub struct Item<T> {
    value: T,
}

impl<T> buffer::raw::Unary<T> for Item<T> {
    fn item(&self) -> &Self {
        self
    }
}

impl<T: buffer::raw::Segment> Item<T> {
    /// Classifies one owner, returning it unchanged when its extent is empty.
    pub fn new(value: T) -> ops::ControlFlow<T, Self> {
        if value.as_ref().is_empty() {
            ops::ControlFlow::Break(value)
        } else {
            // SAFETY: this exact retained owner has a nonempty visible extent.
            ops::ControlFlow::Continue(unsafe { Self::assume(value) })
        }
    }

    /// # Safety
    /// This exact retained owner must expose a nonempty visible extent.
    pub(in crate::buffer::segments) unsafe fn assume(value: T) -> Self {
        Self { value }
    }
}

impl<'d, const N: usize> From<&'d [u8; N]> for Item<resident::Shared<'d>> {
    /// Retains one borrowed array whose length proves at least one byte.
    fn from(bytes: &'d [u8; N]) -> Self {
        const { assert!(N != 0) };
        Self {
            value: resident::Shared::from(bytes.as_slice()),
        }
    }
}

impl<T> Item<T> {
    pub fn into_inner(self) -> T {
        self.value
    }
}

impl<T: buffer::raw::Segment> Item<T> {
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Item {
            value: buffer::View::new(&self.value),
        }
        .try_retain()
    }

    pub(in crate::buffer::segments) fn tail(
        &mut self,
        amount: usize,
    ) -> Option<segments::Tail<'_, T>> {
        segments::Tail::new(&mut self.value, amount)
    }
}

impl<T: AsRef<[u8]>> Item<T> {
    pub(in crate::buffer::segments) fn len(&self) -> NonZero<usize> {
        // SAFETY: every constructor and mutation preserves a nonempty owner.
        unsafe { NonZero::new_unchecked(self.value.as_ref().len()) }
    }

    pub(in crate::buffer::segments) fn chunk(&self) -> buffer::raw::Chunk<'_> {
        // SAFETY: the item retains this complete nonempty borrowed span.
        unsafe { buffer::raw::Chunk::new(self.value.as_ref()) }
    }
}

impl<'source, S: ?Sized> From<&regions::NonEmpty<'_, 'source, S>>
    for Item<buffer::View<'source, S>>
{
    fn from(view: &regions::NonEmpty<'_, 'source, S>) -> Self {
        Self {
            value: *view.view(),
        }
    }
}

impl<'a, T: buffer::raw::Segment> From<&'a segments::Tail<'_, T, NonZero<usize>>>
    for Item<buffer::View<'a, T>>
{
    fn from(tail: &'a segments::Tail<'_, T, NonZero<usize>>) -> Self {
        Self { value: tail.view() }
    }
}

impl<T> AsRef<T> for Item<T> {
    fn as_ref(&self) -> &T {
        &self.value
    }
}

impl<T> ops::Deref for Item<T> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.value
    }
}

impl<T: buffer::raw::Segment> buffer::Residency for Item<T> {
    fn resident(&self) -> usize {
        self.value.resident()
    }
}

impl<'d> From<Item<storage::Shared>> for Item<resident::Shared<'d>> {
    fn from(item: Item<storage::Shared>) -> Self {
        Self {
            value: resident::Shared::from(item.value),
        }
    }
}

impl Item<resident::Shared<'_>> {
    /// Transfers the backing without changing the admitted nonempty byte extent.
    pub fn try_owned(self) -> Result<Item<storage::Shared>, Self> {
        match self.value.try_owned() {
            Ok(value) => Ok(Item { value }),
            Err(value) => Err(Self { value }),
        }
    }
}

impl<'a, T: buffer::raw::Segment> Item<buffer::View<'a, T>> {
    pub(in crate::buffer::segments) fn clamp(
        source: &'a T,
        start: usize,
        end: usize,
    ) -> Option<Self> {
        let bytes = source.as_ref();
        let end = end.min(bytes.len());
        if start >= end {
            return None;
        }
        let bytes = &bytes[start..end];
        Some(Self {
            value: buffer::View { source, bytes },
        })
    }

    pub(in crate::buffer::segments) fn try_retain(self) -> Result<Item<T>, rc::Error> {
        T::try_share(regions::NonEmpty::from(&self))
    }
}

const _: () = {
    assert!(size_of::<Item<resident::Shared<'static>>>() == size_of::<resident::Shared<'static>>());
    assert!(
        align_of::<Item<resident::Shared<'static>>>() == align_of::<resident::Shared<'static>>()
    );
    assert!(
        size_of::<Option<Item<resident::Shared<'static>>>>()
            == size_of::<Option<resident::Shared<'static>>>()
    );
    assert!(size_of::<Item<storage::Shared>>() == size_of::<storage::Shared>());
    assert!(align_of::<Item<storage::Shared>>() == align_of::<storage::Shared>());
    assert!(size_of::<Option<Item<storage::Shared>>>() == size_of::<Option<storage::Shared>>());
    assert!(
        size_of::<Item<buffer::View<'static, storage::Shared>>>()
            == size_of::<buffer::View<'static, storage::Shared>>()
    );
};

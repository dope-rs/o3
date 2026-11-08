use std::{iter, num::NonZero};

pub(in crate::buffer::segments) use join::Relation;
pub(in crate::buffer::segments) use remove::{Multi, Remaining};
use segments::{assembly, assembly::total};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Handle, Key, handles},
    },
    depot::{arena::linked::slots, capacity},
};

pub(in crate::buffer::segments) mod drain;

mod insert;
mod join;
mod remove;

pub(in crate::buffer::segments) struct Single<'a, T, H: Handle<T>> {
    pub(in crate::buffer::segments) sequence: &'a mut Chain<T, H>,
    pub(in crate::buffer::segments) index: slots::Index,
}

impl<T: buffer::raw::Segment, H: Handle<T>> Single<'_, T, H> {
    pub(in crate::buffer::segments::chain::edit) fn get_mut(&mut self) -> &mut segments::Item<T> {
        // SAFETY: Single retains this live index and the sequence's exclusive loan.
        unsafe { &mut *handles::nodes(&self.sequence.handle).value_ptr(self.index) }
    }

    pub(in crate::buffer::segments) fn take(self) -> segments::Item<T> {
        self.sequence.key = Key::EMPTY;
        unsafe { handles::nodes(&self.sequence.handle).detached(self.index) }
            .take()
            .0
    }

    /// Consumes this whole live key: the retained segment is destroyed and its
    /// slot returns to the store, exactly as releasing the taken owner would.
    pub(in crate::buffer::segments) fn discard(self) {
        self.sequence.key = Key::EMPTY;
        // SAFETY: the nonempty key names this live node exclusively, so the
        // detached slot owns its initialized value and its vacant entitlement.
        drop(unsafe { handles::nodes(&self.sequence.handle).detached(self.index) });
    }
}

pub(in crate::buffer::segments) enum Cause {
    Nodes(capacity::Shortage),
    Descriptors(capacity::Shortage),
}

impl Cause {
    pub(in crate::buffer::segments::chain) const ONE_NODE: Self =
        Self::Nodes(capacity::Shortage::ONE);

    pub(in crate::buffer::segments) fn into_error(self) -> segments::Error {
        match self {
            Self::Nodes(cause) => segments::Error::SegmentCapacity(cause),
            Self::Descriptors(cause) => segments::Error::ChainCapacity(cause),
        }
    }

    pub(in crate::buffer::segments) fn allocation(
        self,
        error: crate::alloc::Error,
    ) -> assembly::Error {
        match self {
            Self::Nodes(shortage) => assembly::Error::Nodes {
                shortage,
                allocation: error,
            },
            Self::Descriptors(shortage) => assembly::Error::Descriptors {
                shortage,
                allocation: error,
            },
        }
    }
}

pub(in crate::buffer::segments) struct Growth {
    count: NonZero<u32>,
    len: usize,
    resident: usize,
}

impl Growth {
    pub(in crate::buffer::segments) fn try_pair<T: buffer::raw::Segment>(
        first: &segments::Item<T>,
        second: &segments::Item<T>,
    ) -> Result<Self, total::Error> {
        Self::try_new(
            segments::Count::ONE,
            first.as_ref().as_ref().len(),
            buffer::Residency::resident(first.as_ref()),
            second,
        )
    }

    pub(in crate::buffer::segments::chain) fn try_new<T: buffer::raw::Segment>(
        current: segments::Count,
        len: usize,
        resident: usize,
        item: &segments::Item<T>,
    ) -> Result<Self, total::Error> {
        let added = segments::Count::ONE;
        let count = total::Error::count(current, added)?;
        let added = item.as_ref().as_ref().len();
        let len = total::Error::logical(len, added)?;
        let added = buffer::Residency::resident(item.as_ref());
        let resident = total::Error::resident(resident, added)?;
        Ok(Self {
            // SAFETY: checked addition of one excludes zero.
            count: unsafe { NonZero::new_unchecked(count.into_raw()) },
            len,
            resident,
        })
    }
}

impl buffer::raw::Total for Growth {
    fn count(&self) -> segments::Count {
        segments::Count::new(self.count.get())
    }
    fn len(&self) -> usize {
        self.len
    }
    fn resident(&self) -> usize {
        self.resident
    }
}

impl<A, B> buffer::raw::Total for (A, B, Growth) {
    fn count(&self) -> segments::Count {
        buffer::raw::Total::count(&self.2)
    }
    fn len(&self) -> usize {
        buffer::raw::Total::len(&self.2)
    }
    fn resident(&self) -> usize {
        buffer::raw::Total::resident(&self.2)
    }
}

// SAFETY: Growth::try_pair admitted these two nonempty owners and their exact totals.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T>
    for (segments::Item<T>, segments::Item<T>, Growth)
{
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Item<T>, segments::Item<T>), assembly::Error>;

    fn owners(
        self,
    ) -> (
        impl Iterator<Item = segments::Item<T>>,
        impl buffer::raw::Total,
    ) {
        ([self.0, self.1].into_iter(), self.2)
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new((self.0, self.1), assembly::Error::Metadata(error))
    }
}

// SAFETY: this Many and nonempty item retain at least three owners with checked growth.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T>
    for (segments::Many<T>, segments::Item<T>, Growth)
{
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Many<T>, segments::Item<T>), assembly::Error>;

    fn owners(
        self,
    ) -> (
        impl Iterator<Item = segments::Item<T>>,
        impl buffer::raw::Total,
    ) {
        let mut chain: Chain<T> = self.0.into();
        (
            iter::from_fn(move || chain.pop_item()).chain(iter::once(self.1)),
            self.2,
        )
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new((self.0, self.1), Cause::ONE_NODE.allocation(error))
    }
}

// SAFETY: this nonempty item and Many retain at least three owners with checked growth.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T>
    for (segments::Item<T>, segments::Many<T>, Growth)
{
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Many<T>, segments::Item<T>), assembly::Error>;

    fn owners(
        self,
    ) -> (
        impl Iterator<Item = segments::Item<T>>,
        impl buffer::raw::Total,
    ) {
        let mut chain: Chain<T> = self.1.into();
        (
            iter::once(self.0).chain(iter::from_fn(move || chain.pop_item())),
            self.2,
        )
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new((self.1, self.0), Cause::ONE_NODE.allocation(error))
    }
}

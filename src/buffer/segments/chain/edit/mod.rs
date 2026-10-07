use std::{iter, num::NonZero};

pub(in crate::buffer::segments::chain) use insert::{Back, Edge, Front, Insert};
pub(in crate::buffer::segments) use join::{Join, Relation};
pub(in crate::buffer::segments) use prepend::Prepend;
pub(in crate::buffer::segments) use remove::{Multi, Remaining};
use segments::{assembly, assembly::total};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Handle, Key, chain::Assemble, handles},
    },
    depot::{arena::linked::slots, capacity},
};

pub(in crate::buffer::segments) mod drain;

mod insert;
mod join;
mod prepend;
mod remove;

pub(in crate::buffer::segments) struct Consume<'a, T> {
    head: segments::Front<'a, T>,
    amount: NonZero<usize>,
}

impl<'a, T: buffer::raw::Segment> Consume<'a, T> {
    pub(in crate::buffer::segments) fn new(
        prefix: segments::Drain<'a, segments::Chain<T>>,
    ) -> Option<Self> {
        let amount = NonZero::new(prefix.amount)?;
        Some(Self {
            head: segments::Front {
                sequence: prefix.target,
            },
            amount,
        })
    }

    pub(in crate::buffer::segments) fn commit(mut self) {
        use segments::Tail;

        loop {
            match self.head.position() {
                handles::Occupied::Single(index) => {
                    let mut single = Single {
                        sequence: self.head.sequence,
                        index,
                    };
                    let owner = buffer::raw::IndexedMut::get_mut(&mut single);
                    match Tail::positive(owner, self.amount) {
                        Some(tail) => {
                            tail.commit();
                        }
                        None => single.discard(),
                    }
                    return;
                }
                handles::Occupied::Multi(index) => {
                    let left = Multi {
                        sequence: &mut *self.head.sequence,
                        index,
                    }
                    .consume(self.amount);
                    let Some(amount) = left else {
                        return;
                    };
                    self.amount = amount;
                }
            }
        }
    }
}

pub(in crate::buffer::segments) struct Single<'a, T, H: Handle<T>> {
    pub(in crate::buffer::segments) sequence: &'a mut Chain<T, H>,
    pub(in crate::buffer::segments) index: slots::Index,
}

// SAFETY: every Single construction retains its sequence's exact live single key.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::Indexed<'a> for &'a mut Single<'_, T, H> {
    type Item = segments::Item<T>;

    fn parts(self) -> (&'a slots::Store<Self::Item>, slots::Index) {
        (handles::nodes(&self.sequence.handle), self.index)
    }
}

// SAFETY: the native Single carries the exclusive loan of this same sequence.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::IndexedMut<'a> for &'a mut Single<'_, T, H> {}

impl<T: buffer::raw::Segment, H: Handle<T>> Single<'_, T, H> {
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
        unsafe { handles::nodes(&self.sequence.handle).detached(self.index) }
            .clear()
            .release();
    }
}

pub(in crate::buffer::segments) enum Assembly<T: buffer::raw::Segment> {
    Local,
    Exact {
        item: segments::Item<T>,
        growth: Growth,
        cause: Cause,
    },
}

pub(in crate::buffer::segments) enum Cause {
    Nodes(capacity::Shortage),
    Descriptors(capacity::Shortage),
}

impl Cause {
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
    count: segments::Count,
    len: usize,
    resident: usize,
}

impl Growth {
    pub(in crate::buffer::segments) fn try_pair<T: buffer::raw::Segment>(
        first: &segments::Item<T>,
        second: &segments::Item<T>,
    ) -> Result<Self, total::Error> {
        let current = first.as_ref().as_ref().len();
        let added = second.as_ref().as_ref().len();
        let len = total::Error::logical(current, added)?;
        let current = buffer::Residency::resident(first.as_ref());
        let added = buffer::Residency::resident(second.as_ref());
        let resident = total::Error::resident(current, added)?;
        Ok(Self {
            count: segments::Count::TWO,
            len,
            resident,
        })
    }

    pub(in crate::buffer::segments::chain) fn try_new<T: buffer::raw::Segment, H: Handle<T>>(
        sequence: &Chain<T, H>,
        item: &segments::Item<T>,
    ) -> Result<Self, total::Error> {
        let (current, len, resident) = sequence.totals();
        let added = segments::Count::ONE;
        let count = total::Error::count(current, added)?;
        let added = item.as_ref().as_ref().len();
        let len = total::Error::logical(len, added)?;
        let added = buffer::Residency::resident(item.as_ref());
        let resident = total::Error::resident(resident, added)?;
        Ok(Self {
            count,
            len,
            resident,
        })
    }

    pub(in crate::buffer::segments::chain) fn assemble<
        T: buffer::raw::Segment,
        H: Handle<T>,
        E: Edge,
    >(
        self,
        sequence: &mut Chain<T, H>,
        item: segments::Item<T>,
        edge: E,
    ) -> Assembly<T> {
        match Insert::admit(sequence, item, self, edge) {
            Ok(insert) => {
                insert.commit();
                Assembly::Local
            }
            Err((cause, item, growth)) => Assembly::Exact {
                item,
                growth,
                cause,
            },
        }
    }
}

impl buffer::raw::Total for Growth {
    fn count(&self) -> segments::Count {
        self.count
    }
    fn len(&self) -> usize {
        self.len
    }
    fn resident(&self) -> usize {
        self.resident
    }
}

const _: () = {
    assert!(size_of::<Consume<'static, ()>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Consume<'static, ()>>>() == 2 * size_of::<usize>());
};

impl<A, B> buffer::raw::Total for (A, B, Growth) {
    fn count(&self) -> segments::Count {
        self.2.count
    }
    fn len(&self) -> usize {
        self.2.len
    }
    fn resident(&self) -> usize {
        self.2.resident
    }
}

impl<A, B> buffer::raw::Total for (A, B, Growth, Cause) {
    fn count(&self) -> segments::Count {
        self.2.count
    }
    fn len(&self) -> usize {
        self.2.len
    }
    fn resident(&self) -> usize {
        self.2.resident
    }
}

unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T>
    for (segments::Item<T>, segments::Item<T>, Growth)
{
    type Output = segments::Many<T>;
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Item<T>, segments::Item<T>), assembly::Error>;

    fn owners(self) -> impl Iterator<Item = segments::Item<T>> + buffer::raw::Total {
        buffer::raw::Batch {
            owners: [self.0, self.1].into_iter(),
            total: self.2,
        }
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new((self.0, self.1), assembly::Error::Metadata(error))
    }
}

unsafe impl<T: buffer::raw::Segment, C: Assemble<T>> buffer::raw::Sequence<T>
    for (C, segments::Item<T>, Growth, Cause)
{
    type Output = C;
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(C, segments::Item<T>), assembly::Error>;

    fn owners(self) -> impl Iterator<Item = segments::Item<T>> + buffer::raw::Total {
        let mut chain = self.0.into();
        buffer::raw::Batch {
            owners: iter::from_fn(move || chain.pop_item()).chain(iter::once(self.1)),
            total: self.2,
        }
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new((self.0, self.1), self.3.allocation(error))
    }
}

pub(in crate::buffer::segments) mod flatten;
use std::{iter, mem, num};

use segments::{Chain, Single, assembly};

use crate::{
    buffer,
    buffer::{
        hold, segments,
        segments::{Exact, chain::edit, handles},
        storage,
    },
    mem::rc,
};

/// A chain proved to contain at least two owners.
#[derive(Debug)]
#[repr(transparent)]
pub struct Many<T: buffer::raw::Segment>(pub(in crate::buffer::segments::chain) Chain<T>);

impl<T: buffer::raw::Segment> From<Many<T>> for Chain<T> {
    fn from(val: Many<T>) -> Self {
        val.0
    }
}

impl<T: buffer::raw::Segment> Many<T> {
    /// # Safety
    /// The chain owns a live multi-descriptor containing at least two owners.
    pub(in crate::buffer::segments) const unsafe fn new(chain: Chain<T>) -> Self {
        Self(chain)
    }

    pub const fn as_chain(&self) -> &Chain<T> {
        &self.0
    }

    pub fn len(&self) -> num::NonZero<usize> {
        // SAFETY: Many retains this live descriptor and at least two nonempty owners.
        unsafe { num::NonZero::new_unchecked(self.descriptor().len) }
    }

    pub fn take_front(mut self) -> (T, segments::Nonempty<T>) {
        let (owner, rest) = self.edit().take_front();
        let source = self.0;
        let rest = match rest {
            edit::Remaining::Single => {
                // SAFETY: the completed removal promoted this exact remaining node.
                let single = unsafe { Single::new(source) };
                segments::Nonempty::One(single.into_item())
            }
            edit::Remaining::Multi => segments::Nonempty::Many(Self(source)),
        };
        (owner.into_inner(), rest)
    }

    /// Releases one owner while retaining the remainder's pooled nodes.
    pub fn discard(mut self) -> (segments::Nonempty<T, Single<T>>, hold::Charge) {
        let (charge, rest) = self.edit().discard();
        let source = self.0;
        let rest = match rest {
            edit::Remaining::Single => {
                // SAFETY: the completed removal promoted this exact remaining node.
                segments::Nonempty::One(unsafe { Single::new(source) })
            }
            edit::Remaining::Multi => segments::Nonempty::Many(Self(source)),
        };
        (rest, charge)
    }

    pub fn front(&self) -> &T {
        let head = self.descriptor().links.head_raw();
        // SAFETY: Many retains this descriptor's initialized head in its own node store.
        unsafe { handles::nodes(&self.0.handle).value(head) }.as_ref()
    }

    /// Moves at most one physical owner's prefix, preserving the remaining chain.
    pub fn try_pop(
        mut self,
        limit: usize,
    ) -> Result<(T, segments::Nonempty<T>), (rc::Error, Self)> {
        let mut edit = self.edit();
        let (desc, owner) = edit.front();
        let Some(tail) = owner.tail(limit) else {
            return Ok(self.take_front());
        };
        let prefix = match tail.try_commit() {
            Ok(result) => result,
            Err(error) => return Err((error, self)),
        };
        desc.len -= limit;
        Ok((prefix, segments::Nonempty::Many(self)))
    }

    fn descriptor(&self) -> &handles::Descriptor {
        let sequence = &self.0;
        // SAFETY: Many retains the initialized descriptor selected by its exact multi key.
        unsafe { handles::descriptors(&sequence.handle).value(sequence.key.multi_index()) }
    }

    pub(in crate::buffer::segments::chain) fn edit(
        &mut self,
    ) -> edit::Multi<'_, T, segments::Pool<T>> {
        let sequence = &mut self.0;
        // SAFETY: Many exclusively retains this multi key and at least two initialized items.
        let index = unsafe { sequence.key.multi_index() };
        edit::Multi { sequence, index }
    }
}

impl<T: buffer::raw::Segment> TryFrom<(segments::Item<T>, segments::Item<T>)> for Many<T> {
    type Error = crate::mem::Rejected<(segments::Item<T>, segments::Item<T>), assembly::Error>;

    fn try_from(source: (segments::Item<T>, segments::Item<T>)) -> Result<Self, Self::Error> {
        use edit::Growth;

        let (first, second) = source;
        let growth = match Growth::try_pair(&first, &second) {
            Ok(growth) => growth,
            Err(error) => {
                return Err(crate::mem::Rejected::new(
                    (first, second),
                    assembly::Error::Total(error),
                ));
            }
        };
        Ok(Exact::try_new((first, second, growth))?.collect())
    }
}

impl<T: buffer::raw::Segment> TryFrom<(Many<T>, segments::Item<T>)> for Many<T> {
    type Error = crate::mem::Rejected<(Many<T>, segments::Item<T>), assembly::Error>;
    fn try_from(source: (Many<T>, segments::Item<T>)) -> Result<Self, Self::Error> {
        Self::try_insert::<false>(source.0, source.1)
    }
}

impl<T: buffer::raw::Segment> TryFrom<(segments::Item<T>, Many<T>)> for Many<T> {
    type Error = crate::mem::Rejected<(segments::Item<T>, Many<T>), assembly::Error>;
    fn try_from(source: (segments::Item<T>, Many<T>)) -> Result<Self, Self::Error> {
        Self::try_insert::<true>(source.1, source.0)
            .map_err(|error| error.map(|(chain, item)| (item, chain)))
    }
}

impl<T: buffer::raw::Segment> buffer::raw::Total for &Many<T> {
    fn count(&self) -> segments::Count {
        segments::Count::new(self.descriptor().count.get())
    }
    fn len(&self) -> usize {
        Many::len(self).get()
    }
    fn resident(&self) -> usize {
        self.descriptor().resident_bytes
    }
}

// SAFETY: this borrow retains at least two owners and their exact checked totals.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T> for &Many<T> {
    type Item = Result<segments::Item<T>, rc::Error>;
    type Error = crate::alloc::Error;
    fn owners(self) -> (impl Iterator<Item = Self::Item>, impl buffer::raw::Total) {
        let mut owners = self.0.segments();
        (
            iter::from_fn(move || owners.next_item()).map(segments::Item::try_clone),
            self,
        )
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        error
    }
}

const _: () = {
    assert!(mem::size_of::<Many<storage::Shared>>() == mem::size_of::<Chain<storage::Shared>>());
    assert!(mem::align_of::<Many<storage::Shared>>() == mem::align_of::<Chain<storage::Shared>>());
};

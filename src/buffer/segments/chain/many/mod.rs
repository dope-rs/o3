pub(in crate::buffer::segments) mod flatten;
use std::{iter, mem, num};

use chain::Assemble;
use segments::{Chain, Single, assembly};

use crate::{
    buffer,
    buffer::{
        hold, segments,
        segments::{Exact, chain, chain::edit, handles},
        storage,
    },
    depot::arena::linked::slots,
    mem::rc,
};

/// A chain proved to contain at least two owners.
#[derive(Debug)]
#[repr(transparent)]
pub struct Many<T: buffer::raw::Segment>(Chain<T>);

struct View<'a, T: buffer::raw::Segment>(&'a Many<T>);

// SAFETY: Many retains its own live multi descriptor and nonempty node partition.
unsafe impl<'a, T: buffer::raw::Segment + 'a> buffer::raw::Linked<'a> for View<'a, T> {
    type Item = segments::Item<T>;
    type Metadata = handles::Descriptor;

    fn parts(
        self,
    ) -> (
        &'a slots::Store<Self::Item>,
        &'a slots::Store<Self::Metadata>,
        slots::Index,
    ) {
        let sequence = &self.0.0;
        // SAFETY: Many's constructors retain the exact live multi key.
        let index = unsafe { sequence.key.multi_index() };
        (
            handles::nodes(&sequence.handle),
            handles::descriptors(&sequence.handle),
            index,
        )
    }

    fn head(metadata: &Self::Metadata) -> slots::Index {
        metadata.links.head_raw()
    }
}

impl<T: buffer::raw::Segment> Assemble<T> for Many<T> {
    fn sequence(&mut self) -> &mut segments::Chain<T, segments::Pool<T>> {
        &mut self.0
    }
}

impl<T: buffer::raw::Segment> From<Many<T>> for Chain<T> {
    fn from(val: Many<T>) -> Self {
        val.0
    }
}

impl<T: buffer::raw::Segment> buffer::raw::Assemble<T> for Many<T> {
    fn finish<S: buffer::raw::Sequence<T, Output = Self>>(input: segments::Built<T, S>) -> Self {
        Self(input.chain)
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
        buffer::raw::Linked::front(View(self)).as_ref()
    }

    /// Moves at most one physical owner's prefix, preserving the remaining chain.
    pub fn try_pop(
        mut self,
        limit: usize,
    ) -> Result<(T, segments::Nonempty<T>), (rc::Error, Self)> {
        use segments::Tail;

        let mut edit = self.edit();
        let (desc, owner) = edit.front();
        let Some(tail) = Tail::new(&mut owner.value, limit) else {
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
        buffer::raw::Linked::metadata(View(self))
    }

    fn edit(&mut self) -> edit::Multi<'_, T, segments::Pool<T>> {
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
        Assemble::try_append(source.0, source.1)
    }
}

impl<T: buffer::raw::Segment> TryFrom<(segments::Item<T>, Many<T>)> for Many<T> {
    type Error = crate::mem::Rejected<(segments::Item<T>, Many<T>), assembly::Error>;
    fn try_from(source: (segments::Item<T>, Many<T>)) -> Result<Self, Self::Error> {
        Assemble::try_prepend(source.1, source.0)
    }
}

impl<T: buffer::raw::Segment> TryFrom<(Many<T>, Many<T>)> for Many<T> {
    type Error = crate::mem::Rejected<(Many<T>, Many<T>), assembly::Error>;
    fn try_from(source: (Many<T>, Many<T>)) -> Result<Self, Self::Error> {
        edit::Join::new(source.0, source.1).try_commit()
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

unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T> for &Many<T> {
    type Output = Many<T>;
    type Item = Result<segments::Item<T>, rc::Error>;
    type Error = crate::alloc::Error;
    fn owners(self) -> impl Iterator<Item = Self::Item> + buffer::raw::Total {
        let mut owners = self.0.segments();
        buffer::raw::Batch {
            owners: iter::from_fn(move || owners.next_item()).map(segments::Item::try_clone),
            total: self,
        }
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        error
    }
}

const _: () = {
    assert!(mem::size_of::<Many<storage::Shared>>() == mem::size_of::<Chain<storage::Shared>>());
    assert!(mem::align_of::<Many<storage::Shared>>() == mem::align_of::<Chain<storage::Shared>>());
};

use std::{mem, num::NonZero};

use runtime::capacity::Capacity;

use crate::{
    alloc,
    depot::{
        self, capacity,
        queue::{
            indexed::lanes::{self, runtime},
            links, tagged,
        },
    },
};

/// Exclusive indexed storage whose lane count and endpoint layout are fixed at construction.
#[repr(transparent)]
pub struct Fifo<
    T,
    C: depot::raw::Count = capacity::Count,
    L: depot::raw::Count = capacity::Count,
    H: tagged::raw::Head = links::Chain,
> {
    pub(in crate::depot) block: alloc::raw::Block<Capacity<T, C, L, H>, (), (), lanes::Value<T>>,
}

impl<T, C: depot::raw::Count, L: depot::raw::Count, H: tagged::raw::Head> Unpin
    for Fifo<T, C, L, H>
{
}

impl<T, C: depot::raw::Count, L: depot::raw::Count, H: tagged::raw::Head> Fifo<T, C, L, H> {
    pub fn try_from(capacity: C, lanes: L) -> Result<Self, tagged::Error<C, L>> {
        let shape = tagged::Domain::<[H], C, L>::build(capacity, lanes)?;
        Self::try_new(shape).map_err(Into::into)
    }

    pub fn try_new(shape: tagged::Domain<[H], C, L>) -> Result<Self, alloc::Error> {
        let extent = Capacity::try_new(shape)?;
        Ok(Self {
            block: alloc::raw::Block::try_from_fn(
                extent,
                (),
                |index| {
                    let index = unsafe { links::Index::new_usize(index) };
                    lanes::Value::vacant(index)
                },
                |_| H::default(),
            )?,
        })
    }

    pub fn bucket(&self, hash: u64) -> tagged::Endpoint<&Self> {
        // SAFETY: the mask is below this retained owner's guaranteed nonzero lane count.
        unsafe { tagged::Endpoint::new(self, self.endpoint(hash)) }
    }

    pub fn bucket_mut(&mut self, hash: u64) -> tagged::Endpoint<&mut Self> {
        let endpoint = self.endpoint(hash);
        // SAFETY: the endpoint was derived from this same immutable lane count.
        unsafe { tagged::Endpoint::new(self, endpoint) }
    }

    fn endpoint(&self, hash: u64) -> NonZero<u32> {
        const { assert!(L::MIN != 0) };
        let lane = hash as u32 & (self.lanes().count().raw() - 1);
        // SAFETY: a lane index is below the nonzero u32 count, hence below u32::MAX.
        unsafe { NonZero::new_unchecked(u32::MAX - lane) }
    }

    pub fn take(&mut self, index: usize, expected: T) -> Option<T>
    where
        T: Eq,
    {
        let entry = tagged::Linked::at(self, index)?;
        if entry.get() != &expected {
            return None;
        }
        Some(entry.remove())
    }

    pub(in crate::depot::queue::indexed::lanes::runtime) const fn lanes(&self) -> L {
        self.block.extent().shape().lanes()
    }

    pub(in crate::depot) fn capacity(&self) -> C {
        self.block.extent().shape().capacity()
    }
}

impl<T, C: depot::raw::Count, L: depot::raw::Count, H: tagged::raw::Head> Drop
    for Fifo<T, C, L, H>
{
    fn drop(&mut self) {
        if mem::needs_drop::<T>() {
            tagged::clear(self);
        }
    }
}

const _: () = assert!(mem::size_of::<Fifo<()>>() == 3 * mem::size_of::<usize>());

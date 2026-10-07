use std::{cell, marker, num};

use group::Group;
use lane::Lane;

use crate::{
    alloc,
    base::num::bounded,
    depot::queue::{links, round},
};

pub(super) mod group;
pub(super) mod lane;
pub(in crate::depot::queue::round) mod stores;

/// Stable resident values partitioned among runtime FIFO lanes and active groups.
#[derive(Debug)]
pub struct Lanes<T, const G: usize, const N: usize = 0> {
    pub(in crate::depot::queue::round) block:
        alloc::raw::Block<stores::Capacity<T>, (), alloc::raw::Initialized, stores::Node<T>>,
    pub(in crate::depot::queue::round) groups: [links::Cycle; G],
}

impl<T, const G: usize> Lanes<T, G> {
    pub fn try_new(capacity: usize, lanes: num::NonZero<usize>) -> Result<Self, alloc::Error>
    where
        T: Default,
    {
        let capacity = stores::Capacity::new(capacity, lanes)?;
        Self::allocate(capacity)
    }
}

impl<T, const G: usize, const N: usize> Lanes<T, G, N> {
    /// Allocates the lane extent carried by this owner's type.
    pub fn try_fixed(capacity: usize) -> Result<Self, alloc::Error>
    where
        T: Default,
    {
        const { assert!(N != 0 && N <= u32::MAX as usize, "lanes") };
        let capacity =
            stores::Capacity::new(capacity, num::NonZero::<usize>::MIN.saturating_add(N - 1))?;
        Self::allocate(capacity)
    }

    fn allocate(capacity: stores::Capacity<T>) -> Result<Self, alloc::Error>
    where
        T: Default,
    {
        const { assert!(G != 0, "round lanes require an active group") };
        let block = alloc::raw::Block::try_planes(
            capacity,
            (),
            |index| {
                // SAFETY: Capacity bounds both initialized planes by the Index domain.
                let key = unsafe { links::Index::new_usize(index) };
                (
                    stores::Node {
                        value: T::default(),
                        hook: links::Hook::new(key),
                    },
                    cell::Cell::new(unsafe { links::Index::new_usize(0) }),
                )
            },
            |index| stores::Track::new(unsafe { links::Index::new_usize(index) }),
        )?;
        Ok(Self {
            block,
            groups: [const { links::Cycle::new() }; G],
        })
    }

    /// Admits both positions once and retains the exclusive allocation loan.
    pub fn try_entry(
        &mut self,
        index: usize,
        lane: usize,
    ) -> Result<round::Location<&mut Self>, round::Bounds> {
        let index = self.try_slot(index)?.index;
        let lane = Lane::try_new(&*self, lane)?.index;
        Ok(round::Location::new(self, round::Position { lane, index }))
    }

    pub fn try_slot(&self, index: usize) -> Result<round::Slot<'_, T, G, N>, round::Bounds> {
        let capacity = self.block.as_ref().len();
        if index >= capacity {
            return Err(round::Bounds::Slot { index, capacity });
        }
        Ok(round::Slot {
            owner: self,
            index: unsafe { links::Index::new_usize(index) },
        })
    }

    /// Selects a statically bounded lane without admitting its range again.
    pub fn lane<const MAX: u32>(&self, index: bounded::U32<0, MAX>) -> Lane<&Self> {
        const { assert!((MAX as usize) < N, "lane") };
        Lane {
            owner: self,
            index: unsafe { links::Index::new_usize(index.into_usize()) },
            item: marker::PhantomData,
        }
    }

    pub fn group<const I: usize>(&self) -> Group<'_, T, G, N> {
        const { assert!(I < G, "round group index exceeds the group count") };
        Group {
            owner: self,
            index: I,
            item: marker::PhantomData,
        }
    }
}

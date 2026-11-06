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

type Block<T> =
    alloc::raw::Block<stores::Capacity<T>, (), alloc::raw::Initialized, stores::Node<T>>;

/// Stable resident values partitioned among runtime FIFO lanes and active groups.
#[derive(Debug)]
pub struct Lanes<T, const G: usize, const N: usize = 0> {
    pub(in crate::depot::queue::round) block: Block<T>,
    pub(in crate::depot::queue::round) groups: [links::Cycle; G],
}

/// # Safety
/// Both fields belong to the same live owner with nonzero G; index is an admitted lane.
unsafe fn join<'a, T, const G: usize>(
    block: &'a Block<T>,
    groups: &'a [links::Cycle; G],
    index: links::Index,
) -> round::Join<'a, [stores::Node<T>], [stores::Track]> {
    let lanes = block.tail();
    round::Join {
        hooks: block.as_ref(),
        lanes,
        entries: &unsafe { lanes.get_unchecked(index.get()) }.entries,
        active: &groups[index.get() % G],
        index,
    }
}

/// # Safety
/// Both fields belong to the same live owner with nonzero G. Position retains
/// its admitted resident and destination; lane is its current membership, or
/// None proves detachment. No other topology mutation may intervene until commit.
pub(super) unsafe fn push_from<T, const G: usize>(
    block: &Block<T>,
    groups: &[links::Cycle; G],
    position: round::Position,
    lane: Option<links::Index>,
) {
    let index = position.index;
    let vacant = if let Some(lane) = lane {
        // SAFETY: lane retains this resident's exact membership until removal.
        round::Join::remove(unsafe {
            crate::depot::raw::Proof::new((join(block, groups, lane), index))
        })
    } else {
        // SAFETY: None proves this exact admitted hook is detached.
        unsafe {
            links::Detached::new(
                crate::depot::raw::Hooks::hook_raw(block.as_ref(), index),
                index,
            )
        }
    };
    // SAFETY: position admits the destination against these same-owner fields.
    let target = unsafe { join(block, groups, position.lane) };
    // SAFETY: the detached resident and admitted destination share this owner.
    round::Join::push(unsafe { crate::depot::raw::Proof::new((target, vacant)) });
    // SAFETY: position admits this resident's disjoint ownership cell.
    unsafe { block.values().get_unchecked(index.get()) }.set(position.lane);
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
    /// # Safety
    /// `index` was admitted against this exact owner's lane extent.
    pub(in crate::depot::queue::round) unsafe fn join(
        &self,
        index: links::Index,
    ) -> round::Join<'_, [stores::Node<T>], [stores::Track]> {
        unsafe { join(&self.block, &self.groups, index) }
    }

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

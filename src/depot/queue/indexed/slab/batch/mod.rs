use crate::depot::{array::Index, queue::links};
pub(in crate::depot) mod reserved;
use std::{mem, num::NonZero};

use crate::depot::{
    capacity,
    queue::{
        indexed::{slab, slab::phase},
        tagged,
    },
};

/// Actual distinct vacancies retained under their allocation's exclusive borrow.
pub struct Batch<
    'a,
    T,
    Tag,
    const LANES: usize,
    const N: usize,
    const MAX: u32 = { u32::MAX },
    C: crate::depot::raw::Count = capacity::Count,
> {
    pub(in crate::depot) reserved: slab::Reserved<'a, T, Tag, MAX, tagged::Hook, LANES, N, C>,
}

impl<'a, T, Tag, const LANES: usize, const N: usize, const MAX: u32, C: crate::depot::raw::Count>
    Batch<'a, T, Tag, LANES, N, MAX, C>
{
    pub fn new(
        source: &'a mut slab::Slab<T, Tag, MAX, tagged::Hook, LANES, C>,
    ) -> Result<Self, capacity::Shortage> {
        let free = source.available();
        let reserved = slab::Reserved::new(source).ok_or_else(|| capacity::Shortage {
            // SAFETY: reservation fails only when N exceeds the available count.
            requested: unsafe { NonZero::new_unchecked(N) },
            available: free,
        })?;
        Ok(Self { reserved })
    }

    /// Consumes each reserved slot under its own exclusive iteration borrow.
    pub fn insert<K, I>(
        self,
        keys: [K; N],
        values: [I; N],
        mut insert: impl for<'slot> FnMut(
            slab::Vacant<'slot, T, Tag, MAX, tagged::Hook, LANES, C>,
            K,
            I,
        ),
    ) {
        let (core, slots) = self.reserved.into_parts();
        for ((index, key), value) in slots.into_iter().zip(keys).zip(values) {
            insert(
                slab::Vacant {
                    entry: slab::Slot {
                        core,
                        index,
                        phase: phase::Vacant,
                    },
                },
                key,
                value,
            );
        }
    }
}

impl<'a, T, Tag, const LANES: usize, const MAX: u32, C: crate::depot::raw::Count>
    Batch<'a, T, Tag, LANES, 1, MAX, C>
{
    /// Consumes the single reserved slot without another capacity decision.
    pub fn into_entry(
        self,
        lane: Index<LANES>,
    ) -> slab::Vacant<'a, T, Tag, MAX, tagged::Hook, LANES, C, phase::Target<LANES>> {
        let (core, [index]) = self.reserved.into_parts();
        slab::Vacant {
            entry: slab::Slot {
                core,
                index,
                phase: phase::Vacant,
            },
        }
        .select(lane)
    }
}

const _: () = {
    assert!(mem::size_of::<Batch<'static, u64, (), 1, 1>>() == 2 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Batch<'static, u64, (), 1, 3>>()
            == (mem::size_of::<usize>() + 3 * mem::size_of::<links::Index>())
                .next_multiple_of(mem::align_of::<usize>())
    );
    assert!(
        mem::size_of::<Option<Batch<'static, u64, (), 1, 3>>>()
            == mem::size_of::<Batch<'static, u64, (), 1, 3>>()
    );
};

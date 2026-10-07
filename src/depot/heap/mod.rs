use std::{alloc::Layout, num::NonZero};

pub use entry::{Entry, direct::Direct, stored::Stored, vacancy::Vacancy};

use crate::{
    alloc,
    depot::{capacity::Count, heap, raw::Live},
};

mod entry;

mod capacity;
pub mod hole;
pub mod indexed;
pub mod keyed;
pub mod queue;
pub(in crate::depot) use capacity::Capacity;
pub mod resident;

pub(in crate::depot) mod raw;

/// The plane whose layout extension exceeded the allocation bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Plane {
    Entries,
    Positions,
    Residents,
}

/// Observable effect of one committed heap mutation on its minimum.
#[must_use = "the minimum change must be consumed by the observing policy"]
#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Change {
    Contents,
    Minimum,
}

/// Exact outcome of probing one key for a mutable occupied entry.
#[must_use = "the probed entry state must be consumed by the mutation policy"]
pub enum Probe<T> {
    OutOfRange,
    Vacant,
    Occupied(T),
    Conflict,
}

const _: () = assert!(std::mem::size_of::<Change>() == 1);

/// One admitted physical identity, before comparing its stored generation.
pub(in crate::depot::heap) struct Slot<'a, S: heap::raw::Store> {
    owner: &'a mut S,
    key: S::Key,
    position: S::Position,
}

/// One resolved generation retaining its exact heap and physical slot.
pub(in crate::depot::heap) enum Lookup<'a, S: heap::raw::Store> {
    Vacant(heap::Vacancy<&'a mut S, S::Key>),
    Occupied(heap::Entry<&'a mut S, S::Key>),
    Conflict(heap::Entry<&'a mut S, S::Key>),
}

impl<'a, S: heap::raw::Store> Slot<'a, S> {
    pub(in crate::depot::heap) fn new(owner: &'a mut S, key: S::Key) -> Option<Self> {
        let position = *owner
            .positions()
            .get(crate::depot::raw::Address::index(key))?;
        Some(Self {
            owner,
            key,
            position,
        })
    }

    /// Consumes the original physical admission without repeating its bounds check.
    pub(in crate::depot::heap) fn from_source<C>(
        live: Live<(&'a mut S, C), S::Key, ()>,
    ) -> (Self, C) {
        let ((owner, context), key) = live.into_inner();
        // SAFETY: Live retains this exact initialized reverse-map position and owner.
        let position = unsafe {
            *owner
                .positions()
                .get_unchecked(crate::depot::raw::Address::index(key))
        };
        (
            Self {
                owner,
                key,
                position,
            },
            context,
        )
    }

    pub(in crate::depot::heap) fn map<C, T>(
        self,
        context: C,
        vacant: impl FnOnce(heap::Vacancy<&'a mut S, S::Key>, C) -> T,
        occupied: impl FnOnce(heap::Entry<&'a mut S, S::Key>, C) -> T,
    ) -> T {
        if self.position == <S::Position as crate::depot::raw::Link>::NONE {
            // SAFETY: this absent reverse-map identity leaves one spare entry.
            vacant(unsafe { heap::Vacancy::new(self.owner, self.key) }, context)
        } else {
            // SAFETY: this admitted reverse-map position is occupied.
            occupied(unsafe { self.into_entry() }, context)
        }
    }

    fn occupied(self) -> Option<heap::Entry<&'a mut S, S::Key>> {
        if self.position == <S::Position as crate::depot::raw::Link>::NONE {
            return None;
        }
        // SAFETY: this admitted reverse-map position is occupied.
        Some(unsafe { self.into_entry() })
    }

    /// # Safety
    /// The retained reverse-map position is occupied, rather than NONE.
    unsafe fn into_entry(self) -> heap::Entry<&'a mut S, S::Key> {
        // SAFETY: Slot retains the exact owner and admitted identity, and the
        // occupied reverse map identifies its initialized heap entry.
        unsafe {
            heap::Entry::new(
                self.owner,
                crate::depot::raw::Word::offset(self.position),
                self.key,
            )
        }
    }

    pub(in crate::depot::heap) fn lookup(self) -> Lookup<'a, S> {
        self.map(
            (),
            |entry, ()| Lookup::Vacant(entry),
            |entry, ()| {
                if entry.value().identity == entry.next {
                    Lookup::Occupied(entry)
                } else {
                    Lookup::Conflict(entry)
                }
            },
        )
    }

    pub(in crate::depot::heap) fn replace(self, priority: S::Priority) {
        self.map(
            priority,
            |vacant, priority| vacant.prepare(priority).store(),
            |entry, priority| {
                heap::raw::Hole::prepare(entry.prepare_key(priority));
            },
        );
    }

    pub(in crate::depot::heap) fn clear(self) {
        if let Some(entry) = self.occupied() {
            heap::raw::Hole::retire(entry.into_parts().0);
        }
    }

    pub(in crate::depot::heap) fn retire(self) {
        if let Some(entry) = self.occupied() {
            if entry.value().identity != entry.next {
                return;
            }
            heap::raw::Hole::retire(entry.into_parts().0);
        }
    }
}

impl<'a, S: heap::raw::Store> Lookup<'a, S> {
    pub(in crate::depot::heap) fn try_new(
        owner: &'a mut S,
        key: S::Key,
    ) -> Result<Self, keyed::Error<S::Key>> {
        let capacity = owner.positions().len();
        Ok(Slot::new(owner, key)
            .ok_or(keyed::Error { key, capacity })?
            .lookup())
    }

    pub(in crate::depot::heap) fn from_source<C>(
        live: Live<(&'a mut S, C), S::Key, ()>,
    ) -> (Self, C) {
        let (slot, context) = Slot::from_source(live);
        (slot.lookup(), context)
    }

    pub(in crate::depot::heap) fn probe<T>(
        owner: &'a mut S,
        key: S::Key,
        occupied: impl FnOnce(heap::Entry<&'a mut S, S::Key>) -> T,
    ) -> heap::Probe<T> {
        match Slot::new(owner, key).map(Slot::lookup) {
            None => heap::Probe::OutOfRange,
            Some(Self::Vacant(_)) => heap::Probe::Vacant,
            Some(Self::Occupied(entry)) => heap::Probe::Occupied(occupied(entry)),
            Some(Self::Conflict(_)) => heap::Probe::Conflict,
        }
    }
}

/// Exact failure to construct or grow a heap allocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Error<C = Count> {
    Capacity {
        requested: C,
        maximum: NonZero<usize>,
    },
    Layout {
        capacity: C,
        plane: heap::Plane,
        entries: Layout,
        positions: Layout,
        residents: Layout,
        error: alloc::Overflow,
    },
    Allocation {
        capacity: C,
        layout: Layout,
        error: alloc::Exhausted,
    },
}

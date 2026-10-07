//! Fixed-domain insertion-ordered round-robin membership.

use std::{marker, mem};

pub use lanes::{Lanes, group::Group, lane::Lane};
pub use location::{Location, conflict::Conflict, error::Error, idle::Idle, linked::Linked};
pub use slot::Slot;
pub use snapshot::{Snapshot, front::Front, place::Place, turn::Turn};

use crate::{
    alloc,
    depot::{
        arena::slab::{self, key},
        capacity,
        queue::links,
    },
};

mod join;
pub mod phase;
pub mod set;
pub use join::Join;

/// The first admission failed against this allocation's exact domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Bounds {
    Slot { index: usize, capacity: usize },
    Lane { index: usize, capacity: usize },
}

#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub(in crate::depot::queue::round) struct Position {
    pub lane: links::Index,
    pub index: links::Index,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum Insertion {
    Inserted,
    Present,
}

mod lanes;
mod location;
mod slot;
mod snapshot;
pub mod values;

/// Generation-addressed residents and their insertion-ordered fair schedule.
pub struct Set<T, Tag = (), const MAX: u32 = { u32::MAX }, B = ()> {
    values: slab::Store<T, Tag, MAX>,
    order: set::Order<B>,
}

impl<T, Tag, const MAX: u32> Set<T, Tag, MAX> {
    pub fn try_new(capacity: capacity::Count) -> Result<Self, alloc::Error> {
        Ok(Self {
            values: slab::Store::<T, Tag, MAX>::try_new(capacity)?,
            order: set::Order::try_new(capacity)?,
        })
    }

    pub fn contains(&self, key: key::Handle<Tag, MAX>) -> bool {
        self.values.try_get(key).is_ok()
    }

    /// Validates the complete key once and retains this resident's owner.
    pub fn try_entry(
        &mut self,
        key: key::Handle<Tag, MAX>,
    ) -> Result<set::Occupied<'_, T, Tag, MAX>, key::Error<Tag, MAX>> {
        let entry = self.values.try_entry(key)?;
        Ok(set::Occupied::new(entry, &self.order))
    }
}

impl<T, Tag, const MAX: u32, B> Set<T, Tag, MAX, B> {
    pub fn len(&self) -> capacity::Count {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.len() == capacity::Count::EMPTY
    }

    /// Borrows every resident, including values currently outside the schedule.
    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.values.values()
    }

    pub fn try_vacant(&mut self) -> Result<set::Vacant<'_, T, Tag, MAX, true, B>, slab::Vacancy> {
        Ok(set::Vacant {
            entry: self.values.try_vacant()?,
            order: &self.order,
        })
    }

    /// Borrows each usable resident once; other entries cannot change its membership.
    pub fn entries(&mut self) -> impl Iterator<Item = set::Entry<'_, T, Tag, MAX, B>> {
        let order = &self.order;
        slab::Slots::new(&mut self.values)
            .entries()
            .map(move |entry| match entry {
                slab::Entry::Vacant(entry) => set::Entry::Vacant(set::Vacant { entry, order }),
                slab::Entry::Occupied(entry) => {
                    set::Entry::Occupied(set::Occupied::new(entry, order))
                }
            })
    }

    /// Acquires the scheduled resident directly from this owner's live head.
    pub fn turn(&mut self) -> Option<set::Occupied<'_, T, Tag, MAX, true, B, set::Head>> {
        let index = self.order.ring().first()?;
        // SAFETY: this owner alone creates/removes residents and their membership.
        // A ring head is occupied in values; the exclusive loan covers both stores.
        let live = unsafe { crate::depot::raw::Proof::new(((&mut self.values, ()), index)) };
        let (entry, ()) = slab::Occupied::from_live(live);
        Some(set::Occupied {
            entry,
            order: &self.order,
            index,
            phase: marker::PhantomData,
        })
    }
}

impl<'h, T, Tag, const MAX: u32> Set<T, Tag, MAX, capacity::Domain<'h>> {
    pub fn try_in(domain: capacity::Domain<'h>) -> Result<Self, alloc::Error> {
        Ok(Self {
            values: slab::Store::<T, Tag, MAX>::try_new(domain.count())?,
            order: set::Order::try_new(domain.count())?,
        })
    }

    pub fn contains(&self, key: capacity::Key<'h, key::Handle<Tag, MAX>>) -> bool {
        slab::Slots::contains(unsafe { crate::depot::raw::Proof::new((&self.values, key)) })
    }

    pub fn try_entry(
        &mut self,
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
    ) -> Result<set::Occupied<'_, T, Tag, MAX, true, capacity::Domain<'h>>, key::Stale<'h, Tag, MAX>>
    {
        let entry = slab::Occupied::try_from(unsafe {
            crate::depot::raw::Proof::new((&mut self.values, key))
        })?;
        Ok(set::Occupied::new(entry, &self.order))
    }
}

const _: () = {
    assert!(
        mem::size_of::<Set<()>>()
            == mem::size_of::<slab::Store<()>>() + 2 * mem::size_of::<usize>()
    );
    assert!(mem::size_of::<Option<Set<()>>>() == mem::size_of::<Set<()>>());
};

impl<'h, T, Tag, const MAX: u32> Set<T, Tag, MAX, capacity::Domain<'h>> {
    /// # Safety
    /// The key retains an occupied resident in this exact set and order.
    pub(in crate::depot) unsafe fn project<C>(
        &mut self,
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
        context: C,
    ) -> (
        set::Occupied<'_, T, Tag, MAX, true, capacity::Domain<'h>>,
        C,
    ) {
        let source = self;
        let index = unsafe { links::Index::new_usize(key.as_inner().index() as usize) };
        let live = unsafe { crate::depot::raw::Proof::new(((&mut source.values, context), index)) };
        let (entry, context) = slab::Occupied::from_live(live);
        (set::Occupied::new(entry, &source.order), context)
    }
}

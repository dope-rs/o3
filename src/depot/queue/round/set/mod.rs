//! Exclusive resident capabilities for a fair set.

use crate::{
    alloc,
    depot::{
        arena::slab::{self, entries, key},
        capacity,
        queue::{links, round::Set},
        raw::{Hooks as _, Live, Proof},
    },
};

mod order;
pub(super) use order::Order;

impl<'h, T, Tag, const MAX: u32> Set<'h, T, Tag, MAX> {
    pub fn try_in(domain: capacity::Domain<'h>) -> Result<Self, alloc::Error> {
        Ok(Self {
            values: slab::Store::try_new(domain.count())?,
            order: Order::try_in(domain)?,
        })
    }

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

    pub fn try_vacant(&mut self) -> Result<Vacant<'_, 'h, T, Tag, MAX>, slab::Vacancy> {
        Ok(Vacant {
            entry: self.values.try_vacant()?.into_inner(),
            order: &self.order,
        })
    }

    /// Borrows each usable resident once; other entries cannot change its membership.
    pub fn entries(&mut self) -> impl Iterator<Item = Entry<'_, 'h, T, Tag, MAX>> {
        let order = &self.order;
        self.values.entries().map(move |entry| match entry {
            slab::Entry::Vacant(entry) => Entry::Vacant(Vacant {
                entry: entry.into_inner(),
                order,
            }),
            slab::Entry::Occupied(entry) => {
                Entry::Occupied(Occupied::from_slot(entry.into_inner(), order, ()))
            }
        })
    }

    /// Acquires the scheduled resident directly from this owner's live head.
    pub fn turn(&mut self) -> Option<Occupied<'_, 'h, T, Tag, Head, MAX>> {
        let index = self.order.ring().first()?;
        // SAFETY: the exclusive set loan retains its live, scheduled head.
        Some(unsafe { self.live(index, Head) })
    }

    /// # Safety
    /// The indexed resident is occupied in this set and satisfies the supplied schedule phase.
    unsafe fn live<P>(
        &mut self,
        index: links::Index,
        phase: P,
    ) -> Occupied<'_, 'h, T, Tag, P, MAX> {
        // SAFETY: this projected loan retains the exact initialized resident.
        let live = unsafe { Proof::new(((&mut self.values, ()), index)) };
        let (entry, ()) = slab::Occupied::from_live(live);
        Occupied {
            entry: entry.into_inner(),
            order: &self.order,
            index,
            phase,
        }
    }

    pub fn contains(&self, key: capacity::Key<'h, key::Handle<Tag, MAX>>) -> bool {
        slab::Store::contains(unsafe { Proof::new((&self.values, key)) })
    }

    pub fn try_entry(
        &mut self,
        key: capacity::Key<'h, key::Handle<Tag, MAX>>,
    ) -> Result<Occupied<'_, 'h, T, Tag, (), MAX>, key::Stale<'h, Tag, MAX>> {
        let index = key.slot();
        let entry = slab::Occupied::try_from(unsafe { Proof::new((&mut self.values, key)) })?;
        Ok(Occupied {
            entry: entry.into_inner(),
            order: &self.order,
            index,
            phase: (),
        })
    }
}

/// One independently borrowed physical resident.
pub enum Entry<'a, 'h, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    Vacant(Vacant<'a, 'h, T, Tag, MAX>),
    Occupied(Occupied<'a, 'h, T, Tag, (), MAX>),
}

/// The resident's membership, retained until its consuming transition.
pub enum State<'a, 'h, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    Idle(Occupied<'a, 'h, T, Tag, Idle, MAX>),
    Ready(Occupied<'a, 'h, T, Tag, Ready, MAX>),
}

/// One reserved physical slot that cannot yet appear in the schedule.
pub struct Vacant<'a, 'h, T, Tag = (), const MAX: u32 = { u32::MAX }> {
    entry: entries::Pending<'a, T, MAX>,
    order: &'a Order<'h, Tag>,
}

impl<'a, 'h, T, Tag, const MAX: u32> Vacant<'a, 'h, T, Tag, MAX> {
    pub fn insert(self, value: T) -> Occupied<'a, 'h, T, Tag, Idle, MAX> {
        Occupied::from_slot(self.entry.install(value), self.order, Idle)
    }
}

/// One occupied resident retaining its exact order and exclusive slot loan.
pub struct Occupied<'a, 'h, T, Tag = (), P = (), const MAX: u32 = { u32::MAX }> {
    entry: entries::Busy<'a, T, MAX>,
    order: &'a Order<'h, Tag>,
    index: links::Index,
    phase: P,
}

impl<'a, 'h, T, Tag, P, const MAX: u32> Occupied<'a, 'h, T, Tag, P, MAX> {
    fn from_slot(entry: entries::Busy<'a, T, MAX>, order: &'a Order<'h, Tag>, phase: P) -> Self {
        let index = entry.slot_index();
        Self {
            entry,
            order,
            index,
            phase,
        }
    }

    fn phase<Q>(self, phase: Q) -> Occupied<'a, 'h, T, Tag, Q, MAX> {
        Occupied {
            entry: self.entry,
            order: self.order,
            index: self.index,
            phase,
        }
    }

    pub fn key(&self) -> capacity::Key<'h, key::Handle<Tag, MAX>> {
        let key = key::Handle::new(self.index.raw(), self.entry.generation());
        // SAFETY: this slot and its order were allocated for the same invariant domain.
        capacity::Key::new(unsafe { Proof::new(key) })
    }

    pub fn get(&self) -> &T {
        self.entry.value()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.entry.value_mut()
    }

    pub fn into_mut(self) -> &'a mut T {
        self.entry.into_mut()
    }
}

impl<'a, 'h, T, Tag, const MAX: u32> Occupied<'a, 'h, T, Tag, (), MAX> {
    pub fn state(self) -> State<'a, 'h, T, Tag, MAX> {
        // SAFETY: this resident retains its original order and in-bounds hook.
        let hook = unsafe { self.order.hook_raw(self.index) };
        if self.index.is_linked(hook) {
            State::Ready(self.phase(Ready))
        } else {
            State::Idle(self.phase(Idle))
        }
    }
}

/// An occupied resident proved absent from its owner's schedule.
#[derive(Debug)]
pub struct Idle;

impl<'a, 'h, T, Tag, const MAX: u32> Occupied<'a, 'h, T, Tag, Idle, MAX> {
    pub fn insert(self) -> Occupied<'a, 'h, T, Tag, Ready, MAX> {
        // SAFETY: this capability exclusively owns a vacant hook in this exact order.
        unsafe { self.order.ring().push(self.order, self.index) };
        self.phase(Ready)
    }

    pub fn discard(self) {
        self.entry.discard();
    }

    /// Consumes this exact resident's occupied, unscheduled source plane.
    pub fn from_live<C>(live: Admission<'a, 'h, T, Tag, MAX, C, Self>) -> (Self, C) {
        let ((source, context), key) = live.into_inner();
        // SAFETY: the proof retains the unscheduled resident and the exclusive set loan.
        (unsafe { source.live(key.slot(), Idle) }, context)
    }
}

/// Idle admission retains both the resident identity and the unscheduled owner phase.
type Admission<'a, 'h, T, Tag, const MAX: u32, C, P> =
    Live<(&'a mut Set<'h, T, Tag, MAX>, C), capacity::Key<'h, key::Handle<Tag, MAX>>, P>;

/// An occupied resident proved present in its owner's schedule.
#[derive(Debug)]
pub struct Ready;

impl<'a, 'h, T, Tag, const MAX: u32> Occupied<'a, 'h, T, Tag, Ready, MAX> {
    pub fn remove(self) -> Occupied<'a, 'h, T, Tag, Idle, MAX> {
        // SAFETY: the retained exclusive resident is linked in this exact order.
        unsafe { self.order.ring().unlink_key(self.order, self.index) };
        self.phase(Idle)
    }

    pub fn requeue(self) -> Self {
        let ring = self.order.ring();
        // SAFETY: this live member proves that its exact order remains nonempty.
        let first = unsafe { ring.first_live() };
        // SAFETY: the exclusive resident retains this member and its current front.
        unsafe { ring.move_back::<_, false>(self.order, self.index, first) };
        self
    }
}

/// The exclusively borrowed resident at this owner's current fair turn.
#[derive(Debug)]
pub struct Head;

impl<'a, 'h, T, Tag, const MAX: u32> Occupied<'a, 'h, T, Tag, Head, MAX> {
    pub fn advance(self) -> &'a mut T {
        // SAFETY: the joint owner's head cannot change during this exclusive loan.
        unsafe { self.order.ring().next_live(self.order, self.index) };
        self.into_mut()
    }

    pub fn remove(self) -> Occupied<'a, 'h, T, Tag, Idle, MAX> {
        // SAFETY: this retained resident is the live front of this exact order.
        unsafe { self.order.ring().pop_front(self.order, self.index) };
        self.phase(Idle)
    }
}

/// An occupied resident with an admitted successor generation.
pub type Reuse<'a, 'h, T, Tag = (), const MAX: u32 = { u32::MAX }> =
    Occupied<'a, 'h, T, Tag, key::Generation<MAX>, MAX>;

impl<'a, 'h, T, Tag, const MAX: u32> TryFrom<Occupied<'a, 'h, T, Tag, (), MAX>>
    for Reuse<'a, 'h, T, Tag, MAX>
{
    type Error = Occupied<'a, 'h, T, Tag, (), MAX>;

    fn try_from(entry: Self::Error) -> Result<Self, Self::Error> {
        match entry.entry.generation().checked_add(1) {
            Some(next) => Ok(entry.phase(next)),
            None => Err(entry),
        }
    }
}

impl<'a, 'h, T, Tag, const MAX: u32> Reuse<'a, 'h, T, Tag, MAX> {
    pub fn replace(mut self, value: T) -> (Occupied<'a, 'h, T, Tag, (), MAX>, T) {
        let prior = self.entry.replace(self.phase, value);
        (self.phase(()), prior)
    }
}

const _: () = {
    assert!(size_of::<Set<'static, ()>>() == size_of::<slab::Store<()>>() + 2 * size_of::<usize>());
    assert!(size_of::<Set<'static, u64>>() == 6 * size_of::<usize>());
    assert!(size_of::<Option<Set<'static, ()>>>() == size_of::<Set<'static, ()>>());
    type Member = Occupied<'static, 'static, ()>;
    type Reusable = Reuse<'static, 'static, ()>;
    assert!(size_of::<Member>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<Member>>() == size_of::<Member>());
    assert!(size_of::<Reusable>() == 3 * size_of::<usize>() + 2 * size_of::<u32>());
    assert!(size_of::<Option<Reusable>>() == size_of::<Reusable>());
};

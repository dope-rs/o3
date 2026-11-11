use crate::{
    depot::{
        array::Index,
        queue::{
            group::{self, map},
            indexed::slab::phase,
            links, tagged,
        },
        raw::Live,
    },
    mem::lease,
};

pub struct Item<V, const LANES: usize> {
    pub(super) lane: Index<LANES>,
    pub(super) owner: u64,
    pub(super) value: V,
}

impl<V, const LANES: usize> Item<V, LANES> {
    pub const fn new(lane: Index<LANES>, owner: u64, value: V) -> Self {
        Self { lane, owner, value }
    }

    pub fn into_parts(self) -> (Index<LANES>, u64, V) {
        (self.lane, self.owner, self.value)
    }
}

/// Holds the slot, owner, and predecessor until insertion or cancellation.
pub struct Entry<'a, V, const LANES: usize> {
    slot: Slot<'a, V, LANES>,
    owner: crate::mem::Lease<map::Rollback<'a, LANES>, lease::Open>,
    length: &'a mut u32,
    lane: Index<LANES>,
}

pub(super) enum Slot<'a, V, const LANES: usize> {
    First(group::Vacant<'a, V, LANES, phase::Target<LANES>>),
    Next(Append<'a, V, LANES>),
}

impl<'a, V, const LANES: usize> Entry<'a, V, LANES> {
    pub(super) fn new(
        slot: group::Vacant<'a, V, LANES, phase::Target<LANES>>,
        map: &'a mut map::Tree<LANES>,
        length: &'a mut u32,
        lane: Index<LANES>,
        key: u64,
    ) -> Result<Self, group::Error<LANES>> {
        let owner = map
            .intern(key)
            .map_err(|cause| group::Error::owner(lane, key, cause))?;
        let slot = Slot::new(slot, lane.get(&owner.sink().get().tails).as_ref());
        Ok(Self {
            slot,
            owner,
            length,
            lane,
        })
    }

    pub fn insert(self, value: V) {
        let (mut owner, ()) = self.owner.into_parts();
        let member = owner.node();
        let entry = owner.get_mut();
        let tail = self.slot.insert(member, value, |head| {
            *self.lane.get_mut(&mut entry.heads) = Some(head);
        });
        *self.length += 1;
        *self.lane.get_mut(&mut entry.tails) = Some(tail);
    }
}

impl<'a, V, const LANES: usize> Slot<'a, V, LANES> {
    pub(super) fn new(
        slot: group::Vacant<'a, V, LANES, phase::Target<LANES>>,
        previous: Option<&map::Node<group::EntryTag>>,
    ) -> Self {
        match previous {
            Some(previous) => Self::Next(previous.bind(slot).into()),
            None => Self::First(slot),
        }
    }

    pub(super) fn insert(
        self,
        owner: map::Node<map::Tag>,
        value: V,
        first: impl FnOnce(map::Node<group::EntryTag>),
    ) -> map::Node<group::EntryTag> {
        let record = group::Entry {
            owner,
            value,
            owner_next: None,
        };
        let key = match self {
            Slot::Next(append) => {
                let (key, previous) = append.insert(record);
                previous.owner_next = Some(map::Node::inserted(key));
                key
            }
            Slot::First(slot) => {
                let key = slot.insert(record).key();
                first(map::Node::inserted(key));
                key
            }
        };
        map::Node::inserted(key)
    }
}

/// The next group entry retains its lane, predecessor, and original vacant reservation.
type Append<'a, V, const N: usize> =
    Live<group::Vacant<'a, V, N, phase::Vacant>, (Index<N>, links::Index), tagged::Hook>;

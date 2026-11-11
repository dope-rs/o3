use std::mem;

use crate::depot::{
    array::Index,
    queue::{
        group::{self, map},
        indexed::slab::phase,
        links,
    },
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

/// Holds the slot and owner until insertion or cancellation.
pub struct Entry<'a, V, const LANES: usize> {
    ledger: &'a mut group::Ledger<V, LANES>,
    index: links::Index,
    lane: Index<LANES>,
    owner: map::Node<map::Tag>,
    change: Option<map::Change>,
}

impl<V, const LANES: usize> Drop for Entry<'_, V, LANES> {
    fn drop(&mut self) {
        drop(group::Vacant {
            core: &mut self.ledger.entries,
            index: self.index,
            phase: phase::Target(self.lane),
        });
        if let Some(change) = self.change {
            self.ledger.map.rollback(change);
        }
    }
}

impl<'a, V, const LANES: usize> Entry<'a, V, LANES> {
    pub(super) fn new(
        batch: group::Batch<'a, V, LANES, 1>,
        lane: Index<LANES>,
        key: u64,
    ) -> Result<Self, group::Error<LANES>> {
        let ledger = batch.ledger;
        // SAFETY: the consumed Batch retains its one admitted native slot.
        let slot =
            unsafe { group::batch::Slots::<_, LANES, 1>::new_unchecked(&mut ledger.entries) }
                .into_entry(lane);
        let mut change = None;
        let owner = ledger
            .map
            .intern(key, &mut change)
            .map_err(|cause| group::Error::owner(lane, key, cause))?;
        let (_, index) = slot.into_owner();
        Ok(Self {
            ledger,
            index,
            lane,
            owner,
            change,
        })
    }

    pub fn insert(self, value: V) {
        let slot = group::Vacant {
            core: &mut self.ledger.entries,
            index: self.index,
            phase: phase::Target(self.lane),
        };
        let owner = map::Node::new(self.owner.index());
        let entry = self.ledger.map.get_mut(&owner);
        insert(slot, owner, entry, value);
        *self.lane.get_mut(&mut self.ledger.lane_lengths) += 1;
        mem::forget(self);
    }
}

pub(super) fn insert<V, const LANES: usize>(
    slot: group::Vacant<'_, V, LANES, phase::Target<LANES>>,
    owner: map::Node<map::Tag>,
    entry: &mut map::Entry<LANES>,
    value: V,
) {
    let lane = slot.phase.0;
    let slot = slot.insert(group::Entry {
        owner,
        value,
        owner_next: None,
    });
    let key = slot.key();
    let (core, _) = slot.into_owner();
    match lane
        .get_mut(&mut entry.tails)
        .replace(map::Node::inserted(key))
    {
        Some(previous) => {
            let mut previous: group::Slot<'_, V, LANES> = previous.bind(core).into();
            previous.as_mut().owner_next = Some(map::Node::inserted(key));
        }
        None => *lane.get_mut(&mut entry.heads) = Some(map::Node::inserted(key)),
    }
}

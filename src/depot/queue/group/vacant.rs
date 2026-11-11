use crate::{
    depot::{
        array::Index,
        queue::{
            group::{self, map},
            indexed::slab::phase,
        },
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

/// Holds the slot and owner until insertion or cancellation.
pub struct Entry<'a, V, const LANES: usize> {
    slot: group::Vacant<'a, V, LANES, phase::Target<LANES>>,
    owner: crate::mem::Lease<map::Rollback<'a, LANES>, lease::Open>,
    length: &'a mut u32,
}

impl<'a, V, const LANES: usize> Entry<'a, V, LANES> {
    pub(super) fn new(
        slot: group::Vacant<'a, V, LANES, phase::Target<LANES>>,
        map: &'a mut map::Tree<LANES>,
        length: &'a mut u32,
        key: u64,
    ) -> Result<Self, group::Error<LANES>> {
        let lane = slot.phase.0;
        let owner = map
            .intern(key)
            .map_err(|cause| group::Error::owner(lane, key, cause))?;
        Ok(Self {
            slot,
            owner,
            length,
        })
    }

    pub fn insert(self, value: V) {
        let (mut owner, ()) = self.owner.into_parts();
        let member = owner.node();
        insert(self.slot, member, owner.get_mut(), value);
        *self.length += 1;
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

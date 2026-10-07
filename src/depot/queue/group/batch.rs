use crate::depot::{
    array::Index,
    queue::{
        group::{self, capacity, map, vacant},
        indexed::slab,
    },
};

/// N actual slots reserved before any value is accepted.
pub struct Batch<'a, V, const LANES: usize, const N: usize> {
    slots: slab::Batch<
        'a,
        group::Entry<V>,
        group::EntryTag,
        LANES,
        N,
        { u32::MAX },
        capacity::Count<LANES>,
    >,
    map: &'a mut map::Tree<LANES>,
    lengths: &'a mut [u32; LANES],
}

impl<'a, V, const LANES: usize, const N: usize> Batch<'a, V, LANES, N> {
    pub fn new(ledger: &'a mut group::Ledger<V, LANES>) -> Result<Self, group::Error<LANES>> {
        Ok(Self {
            slots: slab::Batch::new(&mut ledger.entries).map_err(group::Error::Entries)?,
            map: &mut ledger.map,
            lengths: &mut ledger.lane_lengths,
        })
    }

    /// Resolves the whole batch before consuming its commit capability or any value.
    pub fn publish<T, C>(
        self,
        items: [vacant::Item<T, LANES>; N],
        commit: C,
        ready: impl FnOnce(C),
        mut convert: impl FnMut(T) -> V,
    ) -> Result<(), (group::Error<LANES>, Input<C, T, LANES, N>)> {
        let keys = items.each_ref().map(|item| (item.lane, item.owner));
        map::Draft::with(self.map, keys, (commit, items), |map, bound, input| {
            let (commit, items) = input;
            ready(commit);
            self.slots.insert(bound, items, |slot, owner, item| {
                let member = map::Node::inserted(map.key(owner));
                let entry = map.get_mut(owner);
                *item.lane.get_mut(self.lengths) += 1;
                let value = convert(item.value);
                let slot =
                    vacant::Slot::new(slot.select(item.lane), item.lane.get(&entry.tails).as_ref());
                let tail = slot.insert(member, value, |head| {
                    *item.lane.get_mut(&mut entry.heads) = Some(head);
                });
                *item.lane.get_mut(&mut entry.tails) = Some(tail);
            });
        })
    }
}

impl<'a, V, const LANES: usize> Batch<'a, V, LANES, 1> {
    pub fn bind(
        self,
        lane: Index<LANES>,
        owner: u64,
    ) -> Result<vacant::Entry<'a, V, LANES>, group::Error<LANES>> {
        vacant::Entry::new(
            self.slots.into_entry(lane),
            self.map,
            lane.get_mut(self.lengths),
            lane,
            owner,
        )
    }
}

/// A publication retains the commit capability and every original item on failure.
type Input<C, T, const LANES: usize, const N: usize> = (C, [vacant::Item<T, LANES>; N]);

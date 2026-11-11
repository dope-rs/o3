use crate::depot::{
    array::Index,
    queue::{
        group::{self, capacity, map, vacant},
        indexed::slab,
    },
};

/// N slots admitted before any value is accepted.
pub struct Batch<'a, V, const LANES: usize, const N: usize> {
    pub(super) ledger: &'a mut group::Ledger<V, LANES>,
}

impl<'a, V, const LANES: usize, const N: usize> Batch<'a, V, LANES, N> {
    pub fn new(ledger: &'a mut group::Ledger<V, LANES>) -> Result<Self, group::Error<LANES>> {
        Slots::<_, LANES, N>::new(&mut ledger.entries).map_err(group::Error::Entries)?;
        Ok(Self { ledger })
    }

    /// Resolves the whole batch before consuming its commit capability or any value.
    pub fn publish<T, C>(
        self,
        items: [vacant::Item<T, LANES>; N],
        commit: C,
        ready: impl FnOnce(C),
        mut convert: impl FnMut(T) -> V,
    ) -> Result<(), (group::Error<LANES>, Input<C, T, LANES, N>)> {
        let group::Ledger {
            entries,
            map,
            lane_lengths,
        } = self.ledger;
        // SAFETY: Batch retains the exclusive ledger after admitting these N slots.
        let slots = unsafe { Slots::<_, LANES, N>::new_unchecked(entries) };
        let keys = items.each_ref().map(|item| (item.lane, item.owner));
        map::Draft::with(map, keys, (commit, items), |map, bound, input| {
            let (commit, items) = input;
            ready(commit);
            slots.insert(bound, items, |slot, owner, item| {
                let member = map.member(owner);
                let entry = map.get_mut(owner);
                *item.lane.get_mut(lane_lengths) += 1;
                let value = convert(item.value);
                vacant::insert(slot.select(item.lane), member, entry, value);
            });
        })
    }
}

impl<'a, V, const LANES: usize> Batch<'a, V, LANES, 1> {
    #[inline]
    pub fn bind(
        self,
        lane: Index<LANES>,
        owner: u64,
    ) -> Result<vacant::Entry<'a, V, LANES>, group::Error<LANES>> {
        vacant::Entry::new(self, lane, owner)
    }
}

pub(super) type Slots<'a, V, const LANES: usize, const N: usize> = slab::Batch<
    'a,
    group::Entry<V>,
    group::EntryTag,
    LANES,
    N,
    { u32::MAX },
    capacity::Count<LANES>,
>;

/// A publication retains the commit capability and every original item on failure.
type Input<C, T, const LANES: usize, const N: usize> = (C, [vacant::Item<T, LANES>; N]);

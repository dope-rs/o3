use crate::depot::{
    array::Index,
    queue::group::{self, map},
};

/// Retains the owner and its tree position throughout a partial drain.
pub struct Drain<'a, V, const LANES: usize> {
    ledger: &'a mut group::Ledger<V, LANES>,
    owner: map::Node<map::Tag>,
    branch: Option<map::Place>,
    lane: Index<LANES>,
}

/// A resolved record retained under its drain's exclusive loan.
pub struct Item<'a, 'b, V, const LANES: usize> {
    drain: &'a mut Drain<'b, V, LANES>,
    entry: map::Node<group::EntryTag>,
}

impl<'a, V, const LANES: usize> Drain<'a, V, LANES> {
    pub fn new(
        ledger: &'a mut group::Ledger<V, LANES>,
        key: u64,
        lane: Index<LANES>,
    ) -> Option<Self> {
        let (owner, branch) = ledger.map.select(key, lane)?;
        Some(Self {
            ledger,
            owner,
            branch,
            lane,
        })
    }

    pub fn next_entry(&mut self) -> Option<Item<'_, 'a, V, LANES>> {
        let entry = self
            .lane
            .get(&self.ledger.map.get(&self.owner).heads)
            .as_ref()?;
        let entry = map::Node::new(entry.index());
        Some(Item { drain: self, entry })
    }
}

impl<V, const LANES: usize> Drop for Drain<'_, V, LANES> {
    fn drop(&mut self) {
        self.ledger.map.prune_at(&self.owner, self.branch);
    }
}

impl<V, const LANES: usize> Item<'_, '_, V, LANES> {
    pub fn get(&self) -> &V {
        let entry: &group::Entry<V> = self.entry.bind(&self.drain.ledger.entries).into();
        &entry.value
    }

    pub fn remove(self) -> V {
        let drain = self.drain;
        let entry = self.entry.bind(&mut drain.ledger.entries).into().remove();
        drain
            .ledger
            .map
            .get_mut(&drain.owner)
            .detach(drain.lane, entry.owner_next);
        *drain.lane.get_mut(&mut drain.ledger.lane_lengths) -= 1;
        entry.value
    }
}

const _: () = {
    assert!(size_of::<Drain<'static, u64, 2>>() == 3 * size_of::<usize>());
    assert!(size_of::<Item<'static, 'static, u64, 2>>() == 2 * size_of::<usize>());
};

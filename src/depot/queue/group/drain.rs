use crate::{
    depot::{
        array::Index,
        queue::{
            group::{self, capacity, map},
            indexed::slab,
            links, tagged,
        },
    },
    mem::{Lease, Return, lease},
};

/// Retains the owner and its tree position throughout a partial drain.
pub struct Drain<'a, V, const LANES: usize> {
    owner: Lease<Prune<'a, LANES>, lease::Open>,
    entries: &'a mut slab::Slab<
        group::Entry<V>,
        group::EntryTag,
        { u32::MAX },
        tagged::Hook,
        LANES,
        capacity::Count<LANES>,
    >,
    length: &'a mut u32,
    lane: Index<LANES>,
}

struct Prune<'a, const LANES: usize> {
    branch:
        Option<slab::Cursor<'a, map::Branch, map::BranchTag, { u32::MAX }, capacity::Count<LANES>>>,
    root: map::Root<'a>,
}

impl<'a, const LANES: usize> Return for Prune<'a, LANES> {
    type Item = slab::Slot<
        'a,
        map::Entry<LANES>,
        map::Tag,
        { u32::MAX },
        links::Hook,
        0,
        capacity::Count<LANES>,
    >;

    fn return_item(self, owner: Self::Item) {
        if owner.as_ref().heads.iter().all(Option::is_none) {
            self.root.remove(owner, self.branch);
        }
    }
}

/// A resolved record and the exact owner links changed by its removal.
pub struct Item<'a, 'b, V, const LANES: usize> {
    entry: group::Slot<'a, V, LANES>,
    owner: &'a mut slab::Slot<
        'b,
        map::Entry<LANES>,
        map::Tag,
        { u32::MAX },
        links::Hook,
        0,
        capacity::Count<LANES>,
    >,
    length: &'a mut u32,
    lane: Index<LANES>,
}

impl<'a, V, const LANES: usize> Drain<'a, V, LANES> {
    pub fn new(
        ledger: &'a mut group::Ledger<V, LANES>,
        key: u64,
        lane: Index<LANES>,
    ) -> Option<Self> {
        let map::Found {
            owner,
            branch,
            root,
        } = ledger.map.select(key, lane)?;
        Some(Self {
            owner: Lease::open(Prune { branch, root }, owner),
            entries: &mut ledger.entries,
            length: lane.get_mut(&mut ledger.lane_lengths),
            lane,
        })
    }

    pub fn next_entry(&mut self) -> Option<Item<'_, 'a, V, LANES>> {
        let key = self.lane.get(&self.owner.item().as_ref().heads).as_ref()?;
        let entry = key.bind(&mut *self.entries).into();
        Some(Item {
            entry,
            owner: self.owner.item_mut(),
            length: self.length,
            lane: self.lane,
        })
    }
}

impl<V, const LANES: usize> Item<'_, '_, V, LANES> {
    pub fn get(&self) -> &V {
        &self.entry.as_ref().value
    }

    pub fn remove(self) -> V {
        let entry = self.entry.remove();
        self.owner.as_mut().detach(self.lane, entry.owner_next);
        *self.length -= 1;
        entry.value
    }
}

const _: () = {
    assert!(size_of::<Drain<'static, u64, 2>>() == 9 * size_of::<usize>());
    assert!(size_of::<Item<'static, 'static, u64, 2>>() == 5 * size_of::<usize>());
};

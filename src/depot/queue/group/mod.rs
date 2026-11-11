//! Fixed-capacity, ownership-preserving protocol group queues.

use std::mem;

pub use batch::Batch;

use crate::{
    alloc,
    depot::{
        array::Index,
        capacity::Shortage,
        queue::{
            indexed::{slab, slab::phase},
            links, tagged,
        },
    },
};

pub mod capacity;
pub mod drain;
mod map;
pub mod vacant;

#[derive(Clone, Copy, PartialEq, Eq)]
enum EntryTag {}

/// Exact capacity rejection without a partially completed operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<const LANES: usize> {
    Entries(Shortage),
    Owners {
        lane: Index<LANES>,
        capacity: capacity::Count<LANES>,
        key: u64,
    },
    Branches {
        lane: Index<LANES>,
        capacity: capacity::Count<LANES>,
        key: u64,
    },
}

impl<const LANES: usize> Error<LANES> {
    fn owner(lane: Index<LANES>, key: u64, cause: map::Error<LANES>) -> Self {
        match cause {
            map::Error::Owners(capacity) => Self::Owners {
                lane,
                capacity,
                key,
            },
            map::Error::Branches(capacity) => Self::Branches {
                lane,
                capacity,
                key,
            },
        }
    }
}

/// Rejection retains the unqueued value and its exact cause.
pub struct Rejected<V, const LANES: usize> {
    pub error: Error<LANES>,
    pub lane: Index<LANES>,
    pub owner: u64,
    pub value: V,
}

/// Exclusive wire head in its retained ledger.
pub struct Front<'a, V, const LANES: usize> {
    ledger: &'a mut Ledger<V, LANES>,
    entry: map::Node<EntryTag>,
    lane: Index<LANES>,
}

impl<'a, V, const LANES: usize> Front<'a, V, LANES> {
    pub fn get(&self) -> &V {
        let entry: &Entry<V> = self.entry.bind(&self.ledger.entries).into();
        &entry.value
    }

    pub fn remove(self) -> V {
        self.pop().1
    }

    fn pop(self) -> (u64, V) {
        let entry = self.entry.bind(&mut self.ledger.entries).into().remove();
        let owner = self.ledger.map.get(&entry.owner).key;
        self.ledger
            .map
            .remove_member(entry.owner, self.lane, entry.owner_next);
        *self.lane.get_mut(&mut self.ledger.lane_lengths) -= 1;
        (owner, entry.value)
    }

    pub fn take(self) -> (V, Vacancy<'a, V, LANES>) {
        let (entry, slot) = self
            .entry
            .bind(&mut self.ledger.entries)
            .into()
            .take(self.lane);
        let (_, index) = slot.into_owner();
        self.ledger
            .map
            .get_mut(&entry.owner)
            .detach(self.lane, entry.owner_next);
        *self.lane.get_mut(&mut self.ledger.lane_lengths) -= 1;
        (
            entry.value,
            Vacancy {
                ledger: self.ledger,
                index,
                owner: entry.owner,
                lane: self.lane,
            },
        )
    }
}

struct Entry<V> {
    owner: map::Node<map::Tag>,
    value: V,
    owner_next: Option<map::Node<EntryTag>>,
}

/// An extracted value's slot and detached owner affiliation, retained together.
#[must_use]
pub struct Vacancy<'a, V, const LANES: usize> {
    ledger: &'a mut Ledger<V, LANES>,
    index: links::Index,
    owner: map::Node<map::Tag>,
    lane: Index<LANES>,
}

impl<V, const LANES: usize> Drop for Vacancy<'_, V, LANES> {
    fn drop(&mut self) {
        drop(Vacant {
            core: &mut self.ledger.entries,
            index: self.index,
            phase: phase::Restore(self.lane),
        });
        self.ledger.map.prune(&self.owner);
    }
}

impl<V, const LANES: usize> Vacancy<'_, V, LANES> {
    pub fn restore(self, value: V) {
        let slot = Vacant {
            core: &mut self.ledger.entries,
            index: self.index,
            phase: phase::Restore(self.lane),
        };
        let owner = map::Node::new(self.owner.index());
        let (owner, next) = self.ledger.map.restore(owner, self.lane, slot.handle());
        *self.lane.get_mut(&mut self.ledger.lane_lengths) += 1;
        slot.restore(Entry {
            owner,
            value,
            owner_next: next,
        });
        mem::forget(self);
    }
}

impl<V, const LANES: usize> Ledger<V, LANES> {
    pub fn available(&self) -> usize {
        self.entries.available()
    }
    pub fn is_full(&self) -> bool {
        self.entries.is_full()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn lane_len(&self, lane: Index<LANES>) -> usize {
        *lane.get(&self.lane_lengths) as usize
    }

    pub fn front(&self, lane: Index<LANES>) -> Option<&V> {
        Some(&self.entries.front_pair(lane)?.1.value)
    }

    pub fn get(&self, owner: u64, lane: Index<LANES>) -> Option<&V> {
        let owner = self.map.find(owner)?;
        let key = lane.get(&owner.heads).as_ref()?;
        let entry: &Entry<V> = key.bind(&self.entries).into();
        Some(&entry.value)
    }

    pub fn visit_owner(&self, owner: u64, lane: Index<LANES>, mut visit: impl FnMut(&V)) {
        let Some(owner) = self.map.find(owner) else {
            return;
        };
        let mut next = lane.get(&owner.heads).as_ref();
        while let Some(key) = next {
            let entry: &Entry<V> = key.bind(&self.entries).into();
            visit(&entry.value);
            next = entry.owner_next.as_ref();
        }
    }
}

/// Bounded wire lanes and per-owner chains over one canonical value store.
pub struct Ledger<V, const LANES: usize> {
    entries:
        slab::Slab<Entry<V>, EntryTag, { u32::MAX }, tagged::Hook, LANES, capacity::Count<LANES>>,
    map: map::Tree<LANES>,
    lane_lengths: [u32; LANES],
}

impl<V, const LANES: usize> Ledger<V, LANES> {
    pub const fn record_bytes() -> usize {
        mem::size_of::<Entry<V>>()
            + mem::size_of::<slab::Handle<EntryTag>>()
            + mem::size_of::<links::Hook>()
            + map::Tree::<LANES>::record_bytes()
    }

    pub fn try_new(capacity: capacity::Count<LANES>) -> Result<Self, alloc::Error> {
        Ok(Self {
            entries: slab::Slab::try_from(capacity)?,
            map: map::Tree::try_new(capacity)?,
            lane_lengths: [0; LANES],
        })
    }

    pub fn bind_front(&mut self, lane: Index<LANES>) -> Option<Front<'_, V, LANES>> {
        self.select([(lane, ())]).map(|((), front)| front)
    }

    /// Takes the first occupied lane while preserving its caller's policy value.
    pub fn take_first<I, const N: usize>(
        &mut self,
        order: [(Index<LANES>, I); N],
    ) -> Option<(I, (V, Vacancy<'_, V, LANES>))> {
        self.select(order).map(|(item, front)| (item, front.take()))
    }

    fn select<I, const N: usize>(
        &mut self,
        order: [(Index<LANES>, I); N],
    ) -> Option<(I, Front<'_, V, LANES>)> {
        let order = order.map(|(lane, item)| (lane, (lane, item)));
        let ((lane, item), entry) = self.entries.select(order)?;
        let (_, index) = entry.into_owner();
        Some((
            item,
            Front {
                ledger: self,
                entry: map::Node::new(index),
                lane,
            },
        ))
    }

    pub fn push_back(
        &mut self,
        lane: Index<LANES>,
        owner: u64,
        value: V,
    ) -> Result<(), Rejected<V, LANES>> {
        let slot = match Batch::new(self).and_then(|slot| slot.bind(lane, owner)) {
            Ok(slot) => slot,
            Err(error) => {
                return Err(Rejected {
                    error,
                    lane,
                    owner,
                    value,
                });
            }
        };
        slot.insert(value);
        Ok(())
    }

    pub fn pop_front(&mut self, lane: Index<LANES>) -> Option<(u64, V)> {
        self.bind_front(lane).map(Front::pop)
    }

    pub fn drain_owner(
        &mut self,
        owner: u64,
        lane: Index<LANES>,
        mut release: impl FnMut(V),
    ) -> usize {
        let Some(mut drain) = drain::Drain::new(self, owner, lane) else {
            return 0;
        };
        let mut count = 0;
        while let Some(item) = drain.next_entry() {
            release(item.remove());
            count += 1;
        }
        count
    }
}

impl<V> Ledger<V, 1> {
    pub fn clear(&mut self, mut release: impl FnMut(V)) {
        while let Some(((), entry)) = self.entries.select([(Index::at::<0>(), ())]) {
            release(entry.remove().value);
        }
        self.map.clear();
        self.lane_lengths = [0];
    }
}

const _: () = {
    assert!(mem::size_of::<Entry<u64>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Front<'static, u64, 2>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Vacancy<'static, u64, 2>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Index<2>>() == 1);
    assert!(mem::size_of::<Option<Index<2>>>() == 1);
};

mod batch;

/// One initialized slot in this ledger's tagged entry store.
type Slot<'a, V, const N: usize> =
    slab::Slot<'a, Entry<V>, EntryTag, { u32::MAX }, tagged::Hook, N, capacity::Count<N>>;

/// One reserved slot retaining its admission or restoration phase.
type Vacant<'a, V, const N: usize, P> =
    slab::Vacant<'a, Entry<V>, EntryTag, { u32::MAX }, tagged::Hook, N, capacity::Count<N>, P>;

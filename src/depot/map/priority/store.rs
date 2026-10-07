use std::{mem, ptr, slice};

use crate::depot::{
    map,
    map::priority::{self, Plan},
};

const NONE: u32 = u32::MAX;

pub(in crate::depot::map::priority) struct Pair<K, V, P> {
    pub(in crate::depot::map::priority) key: K,
    pub(in crate::depot::map::priority) value: V,
    pub(in crate::depot::map::priority) priority: P,
}

pub(in crate::depot::map::priority) struct Store<K, V, P> {
    nodes: ptr::NonNull<priority::Node<K, V, P>>,
    plan: Plan<K, P, V>,
    next: u32,
    free: u32,
    len: u32,
}

pub(in crate::depot::map::priority) enum Slot {
    Free(u32),
    Fresh(u32),
}

impl Slot {
    pub(in crate::depot::map::priority) fn index(&self) -> u32 {
        match self {
            Self::Free(index) | Self::Fresh(index) => *index,
        }
    }
}

impl<K, V, P> Store<K, V, P> {
    /// # Safety
    /// `nodes` must point to `plan.capacity()` allocated node slots that this
    /// store exclusively owns for its whole lifetime, initialized to `MaybeUninit`.
    pub(in crate::depot::map::priority) const unsafe fn new(
        nodes: ptr::NonNull<priority::Node<K, V, P>>,
        plan: Plan<K, P, V>,
    ) -> Self {
        Self {
            nodes,
            plan,
            next: 0,
            free: NONE,
            len: 0,
        }
    }

    pub(in crate::depot::map::priority) const fn capacity(&self) -> usize {
        self.plan.capacity().get()
    }

    pub(in crate::depot::map::priority) const fn plan(&self) -> Plan<K, P, V> {
        self.plan
    }

    pub(in crate::depot::map::priority) const fn len(&self) -> usize {
        self.len as usize
    }

    pub(in crate::depot::map::priority) fn edit(
        &mut self,
    ) -> (&mut u32, &mut [SlotStorage<K, V, P>]) {
        // SAFETY: this exclusive owner holds the whole `capacity`-wide node
        // allocation, and `SlotStorage` admits uninitialized contents.
        let nodes =
            unsafe { slice::from_raw_parts_mut(self.nodes.as_ptr().cast(), self.capacity()) };
        (&mut self.len, nodes)
    }

    pub(in crate::depot::map::priority) fn vacant(&self) -> Option<Slot> {
        if self.free != NONE {
            return Some(Slot::Free(self.free));
        }
        ((self.next as usize) < self.capacity()).then_some(Slot::Fresh(self.next))
    }

    /// # Safety
    /// `index` must be below `capacity` and name one initialized node.
    pub(in crate::depot::map::priority) unsafe fn pair(&self, index: u32) -> &Pair<K, V, P> {
        // SAFETY: the caller bounds `index` into this owner's node allocation.
        let node = unsafe { &*self.nodes.as_ptr().add(index as usize) };
        // SAFETY: a live node holds one initialized pair at this slot.
        unsafe { node.pair.assume_init_ref() }
    }

    /// # Safety
    /// `index` must be below `capacity` and name one initialized node.
    pub(in crate::depot::map::priority) unsafe fn pair_mut(
        &mut self,
        index: u32,
    ) -> &mut Pair<K, V, P> {
        // SAFETY: the caller bounds `index` into this owner's node allocation; the exclusive borrow keeps it unique.
        let node = unsafe { &mut *self.nodes.as_ptr().add(index as usize) };
        // SAFETY: a live node holds one initialized pair at this slot.
        unsafe { node.pair.assume_init_mut() }
    }

    pub(in crate::depot::map::priority) fn insert(
        &mut self,
        slot: Slot,
        pair: Pair<K, V, P>,
        bucket: &map::raw::Vacant,
    ) -> u32 {
        let index = match slot {
            Slot::Free(index) => {
                // SAFETY: a recycled slot retains its next free index in this allocation.
                self.free = unsafe { (*self.nodes.as_ptr().add(index as usize)).bucket };
                index
            }
            Slot::Fresh(index) => {
                self.next += 1;
                index
            }
        };
        // SAFETY: `slot` proves `index` is a fresh or recycled node of this
        // owner, so the write initializes it exactly once.
        unsafe {
            self.nodes
                .as_ptr()
                .add(index as usize)
                .write(priority::Node {
                    pair: mem::MaybeUninit::new(pair),
                    bucket: bucket.index() as u32,
                    position: NONE,
                });
        }
        index
    }

    /// # Safety
    /// `index` must be below `capacity`, name one initialized pair, and already be absent from the heap.
    pub(in crate::depot::map::priority) unsafe fn remove(&mut self, index: u32) -> Pair<K, V, P> {
        // SAFETY: the caller bounds `index` into this owner's node allocation; the exclusive borrow keeps it unique.
        let node = unsafe { &mut *self.nodes.as_ptr().add(index as usize) };
        // SAFETY: the caller proves the node initialized, so its pair is taken exactly once.
        let pair = unsafe { node.pair.assume_init_read() };
        node.bucket = self.free;
        self.free = index;
        pair
    }

    /// # Safety
    /// `index` must be below `capacity` and name one initialized node.
    pub(in crate::depot::map::priority) unsafe fn bucket(&self, index: u32) -> usize {
        // SAFETY: the caller bounds `index` into this owner's node allocation.
        unsafe { (*self.nodes.as_ptr().add(index as usize)).bucket as usize }
    }

    /// # Safety
    /// `index` must be below `capacity` and name one initialized node.
    pub(in crate::depot::map::priority) unsafe fn set_bucket(
        &mut self,
        index: u32,
        bucket: map::raw::Occupied,
    ) {
        // SAFETY: the caller bounds `index` into this owner's node allocation; the exclusive borrow keeps it unique.
        unsafe { (*self.nodes.as_ptr().add(index as usize)).bucket = bucket.index() as u32 };
    }

    pub(in crate::depot::map::priority) fn clear(&mut self) {
        for index in 0..self.next {
            // SAFETY: `index < next` names one node this store has initialized.
            let node = unsafe { &mut *self.nodes.as_ptr().add(index as usize) };
            if node.position != NONE {
                // SAFETY: a live position marks exactly one pair left to drop.
                unsafe { node.pair.assume_init_drop() };
            }
        }
    }
}

/// Node slots retain their initialization state beside heap positions.
type SlotStorage<K, V, P> = mem::MaybeUninit<priority::Node<K, V, P>>;

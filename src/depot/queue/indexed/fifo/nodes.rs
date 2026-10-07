use std::{cell, mem, ptr};

use crate::{
    alloc::array,
    depot::{capacity, queue::links},
};

/// One stable payload position and its intrusive queue hook.
#[repr(C)]
pub(in crate::depot) struct Slot<T> {
    pub(in crate::depot) value: cell::UnsafeCell<mem::MaybeUninit<T>>,
    pub(in crate::depot) hook: links::Hook,
}

impl<T> Slot<T> {
    pub(in crate::depot) const fn vacant(index: u32, next: u32) -> Self {
        Self {
            value: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
            hook: links::Hook::vacant(index, next),
        }
    }
}

/// Exact storage whose allocation extent and queue state cannot diverge.
#[repr(transparent)]
pub(in crate::depot) struct Nodes<T, State> {
    block: array::Block<Slot<T>, capacity::Count, State>,
}

/// A represented slot borrowed from its exact allocation.
pub(in crate::depot) struct NodeEntry<'a, T, State> {
    store: &'a Nodes<T, State>,
    slot: &'a Slot<T>,
}

impl<T, State> Nodes<T, State> {
    pub(in crate::depot) fn try_count(
        count: capacity::Count,
        state: State,
    ) -> Result<Self, crate::alloc::Error> {
        let block = array::Block::try_new(count, state)?
            .init(|index| Slot::vacant(index as u32, links::Index::END));
        Ok(Self { block })
    }

    pub(in crate::depot) const fn state(&self) -> &State {
        self.block.state()
    }

    pub(in crate::depot) const fn count(&self) -> capacity::Count {
        *self.block.extent()
    }

    pub(in crate::depot) fn get(&self, index: usize) -> Option<NodeEntry<'_, T, State>> {
        self.block
            .get(index)
            .map(|slot| NodeEntry { store: self, slot })
    }

    /// # Safety
    /// The topology owns this represented index in this exact allocation.
    pub(in crate::depot) unsafe fn at(&self, index: links::Index) -> NodeEntry<'_, T, State> {
        NodeEntry {
            store: self,
            // SAFETY: the caller retains the topology's represented-index proof.
            slot: unsafe { self.block.get_unchecked(index.get()) },
        }
    }
}

impl<'a, T, State> NodeEntry<'a, T, State> {
    pub(in crate::depot) fn slot(&self) -> &'a Slot<T> {
        self.slot
    }

    pub(in crate::depot) fn store(&self) -> &'a Nodes<T, State> {
        self.store
    }

    pub(in crate::depot) fn index(&self) -> links::Index {
        // SAFETY: both pointers belong to the same allocation; Slot includes a nonzero hook.
        let index = unsafe { ptr::from_ref(self.slot).offset_from(self.store.block.as_ptr()) };
        // SAFETY: construction bounds this represented slot below the u32 sentinel.
        unsafe { links::Index::new_usize(index as usize) }
    }
}

// SAFETY: Nodes owns an address-stable initialized Slot array; the index
// proof selects one stable hook for every occupied in-bounds position.
unsafe impl<T, State> crate::depot::raw::Hooks for Nodes<T, State> {
    unsafe fn hook_raw(&self, index: links::Index) -> &'_ links::Hook {
        // SAFETY: HookStore's caller proves index lies in this stable Slot extent.
        &unsafe { self.at(index) }.slot.hook
    }
}

const _: () = {
    assert!(size_of::<NodeEntry<'static, u64, ()>>() == 2 * size_of::<usize>());
};

impl<T: Copy + Eq> NodeEntry<'_, T, links::Chain<u32>> {
    pub(in crate::depot) fn replace<const KEEP_EQUAL: bool>(self, value: T) {
        let store = self.store();
        let index = self.index();
        let node = self.slot();
        let change = if node.hook.previous.get() != index.raw() {
            if KEEP_EQUAL {
                let current = *unsafe { (*node.value.get()).assume_init_ref() };
                if current == value {
                    return;
                }
            }
            // SAFETY: the exclusive owner keeps occupancy stable across Eq.
            let vacant = unsafe { store.state().unlink(store, index) };
            unsafe {
                crate::depot::raw::Insert::<u32, false>::from_vacant(store.state(), store, vacant)
            }
        } else {
            // SAFETY: this exclusive resident is idle in its original store.
            unsafe { crate::depot::raw::Insert::<u32, false>::new(store.state(), store, index) }
        };
        unsafe { *node.value.get() = mem::MaybeUninit::new(value) };
        crate::depot::raw::Insert::commit(change);
    }

    pub(in crate::depot) fn unlink(self, expected: T) {
        let store = self.store();
        let index = self.index();
        let node = self.slot();
        if node.hook.previous.get() == index.raw() {
            return;
        }
        let current = *unsafe { (*node.value.get()).assume_init_ref() };
        if current == expected {
            // SAFETY: no alias can mutate this owner during Eq.
            unsafe { store.state().unlink(store, index).idle() };
        }
    }
}

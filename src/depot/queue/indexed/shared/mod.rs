//! Shared copy operations with resident state in the same physical allocation.

use std::{cell, marker, mem, ptr};

pub use bounds::Bounds;

use crate::{
    alloc::array,
    depot::{
        capacity,
        queue::{indexed::shared, links},
    },
};

mod bounds;

/// Copies payloads within one thread, without invoking user code during mutations.
pub struct Fifo<T: Copy, P = (), const N: usize = 1> {
    pub(in crate::depot) nodes: array::Block<Node<T, P, N>, capacity::Count, [links::Chain; N]>,
}

impl<T: Copy, P: Default, const N: usize> Fifo<T, P, N> {
    pub fn try_new(capacity: usize) -> Result<Self, crate::alloc::Error> {
        Self::try_with(capacity, P::default)
    }
}

impl<T: Copy, P, const N: usize> Fifo<T, P, N> {
    pub fn try_with(
        capacity: usize,
        mut seed: impl FnMut() -> P,
    ) -> Result<Self, crate::alloc::Error> {
        const { assert!(N > 0) };
        let count = capacity::Count::try_count(capacity, crate::alloc::Error::Overflow)?;
        let nodes =
            array::Block::try_new(count, [const { links::Chain::new() }; N])?.init(|index| {
                // SAFETY: the constructor excludes the compact sentinel.
                let index = unsafe { links::Index::new_usize(index) };
                Node {
                    state: seed(),
                    values: [const { cell::Cell::new(mem::MaybeUninit::uninit()) }; N],
                    hooks: [(); N].map(|()| links::Hook::new(index)),
                }
            });
        Ok(Self { nodes })
    }
}

impl<T: Copy, P, const N: usize> Fifo<T, P, N> {
    pub fn capacity(&self) -> usize {
        self.nodes.extent().get()
    }

    pub fn is_empty<const Q: usize>(&self) -> bool {
        const { assert!(Q < N) };
        self.nodes.state()[Q].first().is_none()
    }

    pub fn iter(&self) -> impl Iterator<Item = shared::Slot<'_, T, P, N>> {
        self.nodes
            .iter()
            .map(|node| shared::Slot { fifo: self, node })
    }

    /// Looks up an optional physical slot together with all of its memberships.
    pub fn get(&self, index: usize) -> Option<shared::Slot<'_, T, P, N>> {
        let node = self.nodes.get(index)?;
        Some(shared::Slot { fifo: self, node })
    }

    pub fn try_entry(&self, index: usize) -> Result<shared::Slot<'_, T, P, N>, shared::Bounds> {
        self.get(index).ok_or(shared::Bounds {
            index,
            capacity: self.capacity(),
        })
    }

    /// Copies the first payload while retaining its queue membership.
    pub fn front<const Q: usize>(&self) -> Option<T> {
        Some(self.first::<Q>()?.value())
    }

    /// Removes one payload while retaining its physical slot and every other plane.
    pub fn next<const Q: usize>(&self) -> Option<(shared::Slot<'_, T, P, N>, T)> {
        Some(self.first::<Q>()?.remove())
    }

    fn first<const Q: usize>(&self) -> Option<Member<'_, T, P, N, Q, Head>> {
        const { assert!(Q < N) };
        let index = self.nodes.state()[Q].first()?;
        // SAFETY: the head retains this owner's initialized node and selected payload.
        let node = unsafe { self.nodes.get_unchecked(index.get()) };
        Some(Member {
            slot: shared::Slot { fifo: self, node },
            index,
            phase: marker::PhantomData,
        })
    }
}

/// A persistent physical slot and its original allocation borrow.
pub struct Slot<'a, T: Copy, P = (), const N: usize = 1> {
    pub(in crate::depot) fifo: &'a Fifo<T, P, N>,
    pub(in crate::depot) node: &'a Node<T, P, N>,
}

enum Head {}

struct Member<'a, T: Copy, P, const N: usize, const Q: usize, S = ()> {
    slot: Slot<'a, T, P, N>,
    index: links::Index,
    phase: marker::PhantomData<S>,
}

impl<T: Copy, P, const N: usize, const Q: usize, S> Member<'_, T, P, N, Q, S> {
    fn value(&self) -> T {
        // SAFETY: membership is consumed locally without callbacks or escaping references.
        unsafe { self.slot.node.values[Q].get().assume_init() }
    }
}

impl<'a, T: Copy, P, const N: usize, const Q: usize> Member<'a, T, P, N, Q, Head> {
    fn remove(self) -> (Slot<'a, T, P, N>, T) {
        let value = self.value();
        let fifo = self.slot.fifo;
        let hooks = Hooks::<_, _, N, Q>(fifo);
        // SAFETY: only the selected head constructs a Member used for removal.
        crate::depot::raw::Removal::commit(unsafe {
            crate::depot::raw::Pop::at(&fifo.nodes.state()[Q], &hooks, self.index)
        })
        .idle();
        (self.slot, value)
    }
}

impl<'a, T: Copy, P, const N: usize> Slot<'a, T, P, N> {
    pub fn index(&self) -> usize {
        self.position().get()
    }

    pub fn state(&self) -> &'a P {
        &self.node.state
    }

    fn position(&self) -> links::Index {
        // SAFETY: this nonzero-sized node belongs to the retained fixed allocation.
        let index =
            unsafe { ptr::from_ref(self.node).offset_from(self.fifo.nodes.as_ptr().cast()) };
        unsafe { links::Index::new_usize(index as usize) }
    }

    /// Returns an owned snapshot; it does not lend mutable queue storage.
    pub fn get<const Q: usize>(&self) -> Option<T> {
        const { assert!(Q < N) };
        let index = self.position();
        if !index.is_linked(&self.node.hooks[Q]) {
            return None;
        }
        Some(
            Member::<_, _, N, Q> {
                slot: Slot {
                    fifo: self.fifo,
                    node: self.node,
                },
                index,
                phase: marker::PhantomData,
            }
            .value(),
        )
    }

    pub fn push<const Q: usize>(&self, value: T) {
        const { assert!(Q < N) };
        let index = self.position();
        let queue = &self.fifo.nodes.state()[Q];
        let hooks = Hooks::<_, _, N, Q>(self.fifo);
        let change = if index.is_linked(&self.node.hooks[Q]) {
            // SAFETY: membership belongs to this exact owner and plane.
            let vacant = unsafe { queue.unlink(&hooks, index) };
            unsafe { crate::depot::raw::Insert::<(), false>::from_vacant(queue, &hooks, vacant) }
        } else {
            // SAFETY: the retained hook is idle in this exact owner and plane.
            unsafe { crate::depot::raw::Insert::<(), false>::new(queue, &hooks, index) }
        };
        self.node.values[Q].set(mem::MaybeUninit::new(value));
        crate::depot::raw::Insert::commit(change);
    }

    pub fn clear<const Q: usize>(&self) {
        const { assert!(Q < N) };
        let index = self.position();
        if index.is_linked(&self.node.hooks[Q]) {
            // SAFETY: the retained hook is a member of this exact owner and plane.
            unsafe {
                self.fifo.nodes.state()[Q]
                    .unlink(&Hooks::<_, _, N, Q>(self.fifo), index)
                    .idle()
            };
        }
    }
}

pub(in crate::depot) struct Node<T, P, const N: usize> {
    state: P,
    values: [cell::Cell<mem::MaybeUninit<T>>; N],
    hooks: [links::Hook; N],
}

struct Hooks<'a, T: Copy, P, const N: usize, const Q: usize>(&'a Fifo<T, P, N>);

// SAFETY: each fixed allocation owns all chains and each statically selected hook family.
unsafe impl<T: Copy, P, const N: usize, const Q: usize> crate::depot::raw::Hooks
    for Hooks<'_, T, P, N, Q>
{
    unsafe fn hook_raw(&self, index: links::Index) -> &'_ links::Hook {
        const { assert!(Q < N) };
        // SAFETY: every edge was published by a slot admitted by this exact owner.
        &unsafe { self.0.nodes.get_unchecked(index.get()) }.hooks[Q]
    }
}

const _: () = {
    assert!(mem::size_of::<Fifo<u64>>() == 24);
    assert!(mem::size_of::<Fifo<u64, (), 2>>() == 32);
    assert!(mem::size_of::<Slot<'_, u64>>() == 16);
    assert!(mem::size_of::<Option<Slot<'_, u64>>>() == 16);
    assert!(mem::size_of::<Slot<'_, u64, (), 2>>() == 16);
    assert!(mem::size_of::<Option<Slot<'_, u64, (), 2>>>() == 16);
};

use std::num::NonZero;

use o3::{
    alloc::{self, BoxSliceExt as _},
    depot,
    depot::{
        capacity,
        index::Initialized,
        queue::{
            indexed::keyed::{self, Fifo, Link},
            links,
        },
    },
};

use crate::common::depot::index::Id;

pub struct Node {
    pub key: Id,
    link: Link<Id>,
}

// SAFETY: construction pairs each bounded native key with its own stable link.
unsafe impl<'a> depot::raw::KeyBorrow<'a> for &'a Node {
    type Key = Id;
    fn parts(self) -> (Self::Key, &'a Link<Id>) {
        (self.key, &self.link)
    }
}

pub struct Slots {
    nodes: Box<[Node]>,
    fifo: Fifo<Id>,
    target: Fifo<Id>,
}

#[derive(Debug)]
pub enum Error {
    Bounds { index: usize, capacity: usize },
    Linked(Id),
}

impl Slots {
    pub fn queues<const REVERSE: bool>(&mut self) -> Queues<'_> {
        let (fifo, target) = if REVERSE {
            (&mut self.target, &mut self.fifo)
        } else {
            (&mut self.fifo, &mut self.target)
        };
        Queues {
            slots: &self.nodes,
            fifo,
            target,
        }
    }

    pub fn try_seed<const REVERSE: bool>(
        &mut self,
        indices: impl IntoIterator<Item = usize>,
    ) -> Result<(), Error> {
        let mut queues = self.queues::<REVERSE>();
        for index in indices {
            let node = queues.slots.get(index).ok_or(Error::Bounds {
                index,
                capacity: queues.slots.len(),
            })?;
            if node.link.is_linked(node.key) {
                return Err(Error::Linked(node.key));
            }
            // SAFETY: the checked node is canonically detached in this exact root.
            let vacant = keyed::Slot::<_, (), false>::from_source(unsafe {
                depot::raw::Proof::new(Selected {
                    owner: queues,
                    node,
                })
            });
            queues = vacant.push::<false>().release().into_inner();
        }
        Ok(())
    }

    pub fn try_occupied(cap: capacity::Count) -> Result<Self, alloc::Error> {
        Ok(Self {
            nodes: Box::try_from_fn(cap.get(), |index| {
                // SAFETY: this range ends at or below the excluded sentinel.
                let index = unsafe { <links::Index as depot::raw::Word>::from_offset(index) };
                Node {
                    key: Id::new(index.raw(), NonZero::<u32>::MIN),
                    link: Link::from(index),
                }
            })?,
            fifo: Fifo::new(),
            target: Fifo::new(),
        })
    }
}

pub struct Queues<'a> {
    slots: &'a [Node],
    fifo: &'a mut Fifo<Id>,
    target: &'a mut Fifo<Id>,
}

impl<'a> Queues<'a> {
    unsafe fn project_node(&self, index: links::Index) -> &'a Node {
        // SAFETY: callers retain this index's initialization in the original root.
        unsafe { self.slots.get_unchecked(index.get()) }
    }
}

pub struct Selected<'a> {
    owner: Queues<'a>,
    node: &'a Node,
}

impl<'a> Selected<'a> {
    pub fn into_inner(self) -> Queues<'a> {
        self.owner
    }
}

// SAFETY: private construction borrows both FIFOs and their nodes from one owner.
unsafe impl depot::raw::KeySource for Queues<'_> {
    type Key = Id;

    fn fifo(&self) -> &Fifo<Id> {
        self.fifo
    }
}

// SAFETY: retaining this root prevents mutation or replacement of its slots and FIFOs.
unsafe impl depot::raw::KeyRead for Queues<'_> {
    fn entry<'a>(
        live: depot::raw::Proof<(&'a Self, links::Index), Initialized<()>>,
    ) -> keyed::Entry<'a, Id> {
        let (source, index) = live.into_inner();
        // SAFETY: the consumed proof retains this exact initialized slot and source.
        keyed::Entry::from_source(unsafe { source.project_node(index) })
    }
}

// SAFETY: the native reference retains the exact source borrow and initialized slot.
unsafe impl<'a> depot::raw::Project<links::Index> for &'a Queues<'_> {
    type Output = &'a Node;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        unsafe { self.project_node(index) }
    }
}

// SAFETY: selection retains the complete original root in its live witness.
unsafe impl<'a> depot::raw::KeyPlane for Queues<'a> {
    type Slot = Selected<'a>;

    fn select(live: depot::raw::Proof<(Self, links::Index), Initialized<()>>) -> Self::Slot {
        let (owner, index) = live.into_inner();
        // SAFETY: the consumed proof retains this exact initialized slot and root.
        let node = unsafe { owner.project_node(index) };
        Selected { owner, node }
    }
}

// SAFETY: selection retains the complete exclusive root and its initialized
// index; only a detached Route can change the selected FIFO.
unsafe impl depot::raw::KeyOwner for Selected<'_> {
    type Key = Id;
    type Fifo = Fifo<Self::Key>;

    fn parts(&self) -> (Id, &keyed::Link<Id>) {
        depot::raw::KeyBorrow::parts(self.node)
    }

    fn fifo(&self) -> &Fifo<Id> {
        self.owner.fifo
    }

    fn entry<'a>(
        live: depot::raw::Proof<(&'a Self, links::Index), Initialized<()>>,
    ) -> keyed::Entry<'a, Id> {
        let live = unsafe { live.map(|(selected, index)| (&selected.owner, index)) };
        keyed::Entry::from_source(live.into())
    }
}

// SAFETY: swapping the disjoint FIFO loans retains this exact detached slot.
unsafe impl depot::raw::KeyRoute for Selected<'_> {
    type Target = Self;

    unsafe fn route(self) -> Self {
        Self {
            owner: Queues {
                slots: self.owner.slots,
                fifo: self.owner.target,
                target: self.owner.fifo,
            },
            node: self.node,
        }
    }
}

// SAFETY: the original initialized slot and exclusive root remain valid
// in either the linked or detached membership phase.
unsafe impl<const LINKED: bool> depot::raw::KeyFinish<LINKED> for Selected<'_> {
    type Output = Self;

    unsafe fn finish(self) -> Self::Output {
        self
    }
}

// SAFETY: Queues owns disjoint FIFO loans and their one initialized node set.
unsafe impl depot::raw::KeyJoin for Queues<'_> {
    fn target(&self) -> &Fifo<Id> {
        self.target
    }

    fn link(live: depot::raw::Proof<(&Self, links::Index), Initialized<()>>) -> &Link<Id> {
        &live.into().link
    }
}

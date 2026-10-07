use slab::slot;

use crate::depot::{
    capacity,
    queue::{indexed::slab, links},
};

/// Two resolved positions retained under their exact allocation's exclusive borrow.
pub struct Cursor<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    C: crate::depot::raw::Count = capacity::Count,
> {
    pub(in crate::depot) entry: slab::Slot<'a, T, Tag, MAX, links::Hook, 0, C>,
    pub(in crate::depot) parent: Option<links::Index>,
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count> Cursor<'a, T, Tag, MAX, C> {
    pub(in crate::depot) fn new(entry: slab::Slot<'a, T, Tag, MAX, links::Hook, 0, C>) -> Self {
        Self {
            entry,
            parent: None,
        }
    }

    pub fn try_insert(
        self,
        build: impl FnOnce(Option<&mut T>, slab::Handle<Tag, MAX>) -> T,
    ) -> Result<Self, C> {
        let parent = Some(self.entry.index);
        self.splice(parent, build)
    }

    pub fn try_prepend(
        self,
        build: impl FnOnce(Option<&mut T>, slab::Handle<Tag, MAX>) -> T,
    ) -> Result<Self, C> {
        let parent = self.parent;
        self.splice(parent, build)
    }

    fn splice(
        self,
        parent: Option<links::Index>,
        build: impl FnOnce(Option<&mut T>, slab::Handle<Tag, MAX>) -> T,
    ) -> Result<Self, C> {
        let (core, _) = self.entry.into_owner();
        let capacity = core.capacity();
        let Some(slot) = core.vacant() else {
            return Err(capacity);
        };
        // SAFETY: Cursor owns its initialized parent in the same allocation;
        // the newly reserved slot is distinct and retained until insertion.
        let splice = match parent {
            Some(parent) => {
                slot::Splice::Parent(unsafe { crate::depot::raw::Proof::new((slot, parent)) })
            }
            None => slot::Splice::Root(slot),
        };
        Ok(Self {
            entry: splice.insert(build),
            parent,
        })
    }

    pub fn remove<R>(self, unlink: impl FnOnce(Option<&mut T>, T) -> R) -> R {
        let (value, slot) = self.entry.detach().into_spare();
        // SAFETY: suspension retains the exact allocation and reserved slot;
        // Cursor's distinct initialized parent remains live until unlink commits.
        let splice = match self.parent {
            Some(parent) => {
                slot::Splice::Parent(unsafe { crate::depot::raw::Proof::new((slot, parent)) })
            }
            None => slot::Splice::Root(slot),
        };
        splice.remove(value, unlink)
    }
}

const _: () = assert!(size_of::<Cursor<'static, u64>>() == 3 * size_of::<usize>());

use slab::phase;

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
    P: phase::State = phase::Occupied,
> {
    entry: slab::Slot<'a, T, Tag, MAX, links::Hook, 0, C, P>,
    parent: Option<links::Index>,
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count, P: phase::State>
    Cursor<'a, T, Tag, MAX, C, P>
{
    /// # Safety
    /// A present parent is initialized in this entry's exact owner and distinct
    /// from the entry; it remains live until this cursor is consumed.
    pub(in crate::depot) unsafe fn new(
        entry: slab::Slot<'a, T, Tag, MAX, links::Hook, 0, C, P>,
        parent: Option<links::Index>,
    ) -> Self {
        Self { entry, parent }
    }
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count>
    Cursor<'a, T, Tag, MAX, C, phase::Vacant>
{
    pub(in crate::depot) fn key(&self) -> slab::Handle<Tag, MAX> {
        self.entry.handle()
    }

    pub(in crate::depot) fn parent_mut(&mut self) -> Option<&mut T> {
        let index = self.parent?;
        Some(
            slab::Slot {
                core: &mut *self.entry.core,
                index,
                phase: phase::Occupied,
            }
            .into_mut(),
        )
    }

    pub(in crate::depot) fn into_inner(self) -> slab::Vacant<'a, T, Tag, MAX, links::Hook, 0, C> {
        self.entry
    }
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count> Cursor<'a, T, Tag, MAX, C> {
    pub fn remove<R>(self, unlink: impl FnOnce(Option<&mut T>, T) -> R) -> R {
        let (value, entry) = self.entry.detach().into_spare();
        let mut cursor = Cursor {
            entry,
            parent: self.parent,
        };
        let result = unlink(cursor.parent_mut(), value);
        cursor.into_inner().cancel();
        result
    }
}

const _: () = assert!(size_of::<Cursor<'static, u64>>() == 3 * size_of::<usize>());

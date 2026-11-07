use crate::depot::{
    index::Initialized,
    queue::{links, tagged},
};

/// Exclusive live membership, with the original initialized entry intact.
pub struct Linked<E: crate::depot::raw::TagEntry> {
    entry: E,
}

impl<E: crate::depot::raw::TagEntry> Linked<E> {
    /// # Safety
    /// The entry was obtained from its live lane or just inserted by Vacant.
    pub(in crate::depot) unsafe fn new(entry: E) -> Self {
        Self { entry }
    }

    pub fn entry(&self) -> &E {
        &self.entry
    }

    pub fn into_inner(self) -> E {
        self.entry
    }

    /// # Safety
    /// The caller may change only the payload, preserving the exact entry and its hook.
    pub(in crate::depot) unsafe fn entry_mut(&mut self) -> &mut E {
        &mut self.entry
    }
}

impl<'a, S: crate::depot::raw::LaneStore>
    Linked<crate::depot::raw::Proof<(&'a mut S, links::Index), Initialized<links::Chain>>>
{
    pub(in crate::depot) fn at(store: &'a mut S, index: usize) -> Option<Self> {
        let node = store.nodes().get(index)?;
        // SAFETY: this exact owner's immutable node extent admits the index.
        let index = unsafe { links::Index::new_usize(index) };
        if !node.hook.is_linked(index) {
            return None;
        }
        // SAFETY: the checked live hook retains its initialized payload under
        // the original exclusive owner borrow through removal or release.
        let entry = unsafe { crate::depot::raw::Proof::new((store, index)) };
        Some(unsafe { Self::new(entry) })
    }
}

impl<E: crate::depot::raw::TagEntry> tagged::Linked<E>
where
    E::Store: crate::depot::raw::LaneStore,
{
    pub(in crate::depot) fn get(&self) -> &<E::Store as crate::depot::raw::LaneStore>::Item {
        // SAFETY: the Store contract couples this live membership to its payload.
        unsafe {
            (*<E::Store as crate::depot::raw::LaneStore>::node(self.entry())
                .value
                .get())
            .assume_init_ref()
        }
    }

    pub fn remove(self) -> <E::Store as crate::depot::raw::LaneStore>::Item {
        // SAFETY: unlink retains this exact initialized payload under its exclusive owner.
        unsafe { self.unlink().take() }
    }
}

use crate::depot::queue::{links, tagged};

/// # Safety
/// Projections retain one exact live hook; its membership cannot change independently.
/// Projections return normally without reentering the topology.
pub unsafe trait TagEntry {
    /// # Safety
    /// The retained entry is live in its lane; implementations preserve this exact entry.
    unsafe fn linked(self) -> tagged::Linked<Self>
    where
        Self: Sized,
    {
        unsafe { tagged::Linked::new(self) }
    }
    /// # Safety
    /// The initialized entry is detached; implementations preserve this exact entry.
    unsafe fn vacant(self) -> tagged::Vacant<Self>
    where
        Self: Sized,
    {
        unsafe { tagged::Vacant::new(self) }
    }
    /// # Safety
    /// Mutations preserve the exact entry and hook; implementations return its retained entry.
    unsafe fn linked_mut<P: crate::depot::raw::TagFinish<Self>>(
        state: &mut tagged::Linked<Self, P>,
    ) -> &mut Self
    where
        Self: Sized,
    {
        unsafe { state.entry_mut() }
    }

    /// tagged::Linked publication is valid only after the payload is initialized.
    const INITIALIZED: bool = true;
    type Store: crate::depot::raw::TagStore;

    fn store(&self) -> &Self::Store;
    fn hook(&self) -> &tagged::Hook;
    fn index(&self) -> links::Index;
}

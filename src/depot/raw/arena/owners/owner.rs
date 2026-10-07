use crate::depot::arena::linked::{fifo::ends, slots};

/// # Safety
/// Links and detachment must retain the exact store, live front, and sole return entitlement.
pub unsafe trait SlotOwner<Tag> {
    fn links(&self) -> &ends::Exclusive<slots::Index<Tag>>;

    /// # Safety
    /// `index` is the owner's live front in this exact `store`, and its links
    /// are reciprocal with that store's nodes.
    unsafe fn detach<'a, T>(
        &mut self,
        store: &'a slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, &'a slots::Store<T>, Tag>;
}

use crate::depot::queue::indexed::lanes;

/// Co-located initialized hooks and phase-controlled affine payloads.
/// # Safety
/// Hooks match nodes; linked hooks own initialized payloads, and entries exclude mutation.
pub unsafe trait LaneStore: crate::depot::raw::TagStore {
    type Item;
    type Publish<E: crate::depot::raw::TagEntry<Store = Self>>: crate::depot::raw::TagFinish<E>;
    fn nodes(&self) -> &[lanes::Value<Self::Item>];

    fn node<'a, E: crate::depot::raw::TagEntry<Store = Self>>(
        entry: &'a E,
    ) -> &'a lanes::Value<Self::Item>
    where
        Self: Sized + 'a,
    {
        // SAFETY: Entry retains this exact owner's represented initialized hook.
        unsafe { Self::nodes(entry.store()).get_unchecked(entry.index().get()) }
    }
}

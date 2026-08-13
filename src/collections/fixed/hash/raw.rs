use crate::collections::fixed::hash;

pub trait Map<V> {
    /// # Safety
    /// The matching entry must exist or the map must have spare capacity.
    unsafe fn entry_unchecked<'map>(
        &'map mut self,
        hash: u64,
        matches: impl FnMut(&V) -> bool,
    ) -> hash::Entry<'map, V>;

    /// # Safety
    /// The matching entry must exist.
    unsafe fn occupied_entry_unchecked<'map>(
        &'map mut self,
        hash: u64,
        matches: impl FnMut(&V) -> bool,
    ) -> hash::Occupied<'map, V>;
}

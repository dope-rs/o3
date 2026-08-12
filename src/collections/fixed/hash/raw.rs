use std::hint;

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

impl<V> Map<V> for hash::Map<V> {
    unsafe fn entry_unchecked<'map>(
        &'map mut self,
        hash: u64,
        matches: impl FnMut(&V) -> bool,
    ) -> hash::Entry<'map, V> {
        match self.probe(hash, matches) {
            hash::Probe::Occupied(index) => {
                hash::Entry::Occupied(hash::Occupied { map: self, index })
            }
            hash::Probe::Vacant(index) => hash::Entry::Vacant(hash::Vacant {
                map: self,
                index,
                hash,
            }),
            hash::Probe::Full => unsafe { hint::unreachable_unchecked() },
        }
    }

    unsafe fn occupied_entry_unchecked<'map>(
        &'map mut self,
        hash: u64,
        matches: impl FnMut(&V) -> bool,
    ) -> hash::Occupied<'map, V> {
        match self.probe(hash, matches) {
            hash::Probe::Occupied(index) => hash::Occupied { map: self, index },
            hash::Probe::Vacant(_) | hash::Probe::Full => unsafe { hint::unreachable_unchecked() },
        }
    }
}

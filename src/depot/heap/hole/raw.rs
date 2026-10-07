use std::cmp;

use crate::depot::heap::{self, hole};

/// # Safety
/// Retain the exclusive owner, layout, ordering domain, initialized prefix and reverse map; consume each proof once.
/// Comparisons preserve storage and membership; memory safety permits inconsistent ordering. Mutations never reenter or expose holes.
pub unsafe trait Source: Sized {
    type Value;

    fn len(&self) -> u32;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// # Safety
    /// Restore the initialized prefix and reverse map before releasing the owner.
    unsafe fn len_mut(&mut self) -> &mut u32;

    /// Compares the held replacement against this initialized position.
    fn compare(entry: hole::At<&Self, &Self::Value>) -> cmp::Ordering;
    /// Compares a replacement whose encoded identity is unchanged.
    fn direction(entry: hole::At<&Self, &Self::Value>) -> cmp::Ordering {
        Self::compare(entry)
    }
    fn compare_at(left: hole::At<&Self>, right: hole::At<&Self>) -> cmp::Ordering;
    /// Transfers this initialized value; REMOVE ends this member's heap ordering.
    fn read<const REMOVE: bool>(entry: hole::At<&mut Self>) -> Self::Value;

    /// # Safety
    /// This exclusive owner proves position < len and accepts the input in its stable ordering domain.
    unsafe fn entry<V>(self, position: usize, value: V) -> heap::Entry<Self, V> {
        unsafe { heap::Entry::new(self, position, value) }
    }

    /// # Safety
    /// This exclusive owner has spare capacity, length below u32::MAX, and accepts this absent identity.
    unsafe fn vacancy(self, value: Self::Value) -> heap::Vacancy<Self, Self::Value> {
        unsafe { heap::Vacancy::new(self, value) }
    }

    /// # Safety
    /// The replacement preserves encoded identity and ancillary records when it stays at this position.
    unsafe fn update(entry: heap::Entry<Self, Self::Value>) {
        heap::raw::Hole::update(entry);
    }
    /// Moves the initialized source into the hole and records its new reverse position.
    fn shift(step: hole::Step<'_, Self>);
    /// Initializes the final hole and returns its exact owner.
    /// Without PLACE, the identity and its ancillary records are already in place.
    fn write<const PLACE: bool>(finish: hole::Finish<Self>) -> Self;
}

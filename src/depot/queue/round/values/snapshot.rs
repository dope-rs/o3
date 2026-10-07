use std::cell::Cell;

use crate::depot::{
    self,
    queue::round::{
        self,
        values::{Head, Slot},
    },
};

/// A revocable snapshot of a closed payload owner's admitted resident and lane.
pub struct Snapshot<'a, T: Copy, const G: usize, P, const N: usize = 0>(
    pub(super) round::Snapshot<&'a round::Lanes<Cell<Option<T>>, G, N>, P>,
);

impl<'a, T: Copy, const G: usize, P, const N: usize> Snapshot<'a, T, G, P, N> {
    /// Copies the admitted payload before reentry, then revalidates through the same owner.
    fn filter<const FAIR: bool>(&self, test: impl FnOnce(T) -> bool) -> Option<Head<'a, T, G, N>> {
        self.0
            .filter::<FAIR>(|head| test(Head(head).get()))
            .map(Head)
    }
}

impl<'a, T: Copy, const G: usize, const N: usize> Snapshot<'a, T, G, round::Place, N> {
    pub fn slot(&self) -> Slot<'a, T, G, N> {
        Slot(self.0.slot())
    }

    /// Schedules an accepted payload after checking equality and current initialization.
    /// Returns the callback's context, preserving queue order when already present.
    /// Equality may reenter, so initialization is checked again before insertion.
    pub fn set<R>(&self, test: impl FnOnce(T) -> Option<R>) -> Option<(round::Insertion, R)>
    where
        T: PartialEq,
    {
        let entry = self.0.slot().get();
        let before = entry.get()?;
        let result = test(before)?;
        if entry.get() != Some(before) {
            return None;
        }
        entry.get()?;
        Some((self.0.set(), result))
    }

    /// Replaces this payload and moves it to this destination's tail.
    pub fn push(&self, value: T) {
        self.0.slot().get().set(Some(value));
        self.0.push();
    }
}

impl<T: Copy, const G: usize, const N: usize> Snapshot<'_, T, G, round::Front, N> {
    pub fn pop_if(self, test: impl FnOnce(T) -> bool) -> Option<T> {
        Some(self.filter::<false>(test)?.pop())
    }
}

impl<T: Copy, const G: usize, const N: usize> Snapshot<'_, T, G, round::Turn, N> {
    pub fn pop_if(self, test: impl FnOnce(T) -> bool) -> Option<T> {
        let head = self.filter::<true>(test)?;
        // SAFETY: the callback ended and both exact front memberships were revalidated.
        Some(Head::consume(unsafe { depot::raw::Proof::new(head) }))
    }
}

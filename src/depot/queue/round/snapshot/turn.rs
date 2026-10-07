use std::mem;

use crate::depot::queue::round;

/// One owner-bound snapshot of the fair turn in an active group.
#[derive(Debug)]
pub struct Turn;

impl<'a, T, const G: usize, const N: usize> round::Snapshot<&'a round::Lanes<T, G, N>, Turn> {
    pub fn get(&self) -> Option<&'a T> {
        Some(self.turn()?.get())
    }

    /// Commits only while this remains the fair turn after the predicate.
    pub fn pop_if(self, test: impl FnOnce(&'a T) -> bool) -> Option<(usize, &'a T)> {
        let head = self.filter::<true>(|head| test(head.get()))?;
        // SAFETY: filter revalidated both the exact FIFO front and fair group
        // turn after the callback; no callback intervenes before this removal.
        let value = round::Location::take_active(unsafe { crate::depot::raw::Proof::new(head) });
        Some((self.lane(), value))
    }
}

const _: () = assert!(
    mem::size_of::<round::Snapshot<&'static round::Lanes<(), 1>, Turn>>()
        == 2 * mem::size_of::<usize>()
);

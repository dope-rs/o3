use std::mem;

use crate::depot::queue::round;

/// One owner-bound snapshot of an exact lane front.
#[derive(Debug)]
pub struct Front;

impl<'a, T, const G: usize, const N: usize> round::Snapshot<&'a round::Lanes<T, G, N>, Front> {
    pub fn get(&self) -> Option<&'a T> {
        Some(self.resolve()?.get())
    }

    /// Removes this snapshot if it still names the exact front.
    pub fn pop(self) -> Option<&'a T> {
        Some(self.resolve()?.pop())
    }

    /// Commits only while this remains the exact front after the predicate.
    pub fn pop_if(self, test: impl FnOnce(&'a T) -> bool) -> Option<&'a T> {
        Some(self.filter::<false>(|head| test(head.get()))?.pop())
    }
}

const _: () = assert!(
    mem::size_of::<round::Snapshot<&'static round::Lanes<(), 1>, Front>>()
        == 2 * mem::size_of::<usize>()
);

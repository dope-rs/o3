mod path;
pub mod raw;
pub(in crate::depot) use path::Path;

use crate::depot::heap;

/// One initialized position projected while the engine retains its complete owner.
pub struct At<S, V = ()> {
    owner: S,
    position: usize,
    next: V,
}

impl<S, V> At<S, V> {
    pub(in crate::depot) fn new(owner: S, position: usize, next: V) -> Self {
        Self {
            owner,
            position,
            next,
        }
    }

    pub fn into_inner(self) -> (S, usize, V) {
        (self.owner, self.position, self.next)
    }
}

/// One initialized source moving into the exact hole under its exclusive owner loan.
pub struct Step<'a, S: raw::Source> {
    pub(in crate::depot) hole: &'a mut heap::raw::Hole<S>,
    pub(in crate::depot) source: usize,
}

impl<'a, S: raw::Source> Step<'a, S> {
    pub(in crate::depot) fn shift(self) {
        let Self { hole, source } = self;
        S::shift(Step {
            hole: &mut *hole,
            source,
        });
        hole.position = source;
    }

    /// Consumes this move into its retained owner, initialized source, and vacant target.
    pub fn into_inner(self) -> (&'a mut S, usize, usize) {
        (&mut self.hole.owner, self.source, self.hole.position)
    }
}

/// The final hole and its owned value, ready to restore the initialized prefix.
pub struct Finish<S: raw::Source> {
    pub(in crate::depot) hole: heap::raw::Hole<S>,
}

impl<S: raw::Source> Finish<S> {
    pub fn into_inner(self) -> (S, usize, S::Value) {
        let heap::raw::Hole {
            owner,
            position,
            value,
        } = self.hole;
        (owner, position, value)
    }
}

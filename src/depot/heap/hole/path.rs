use std::cmp;

use crate::depot::heap::{self, hole};

/// A completed ordering decision retaining its original exclusive owner.
pub(in crate::depot) struct Path<S> {
    pub(in crate::depot) owner: S,
    start: usize,
    end: usize,
}

impl<S: hole::raw::Source> Path<S> {
    pub(in crate::depot) fn at(owner: S, position: usize) -> Self {
        Self {
            owner,
            start: position,
            end: position,
        }
    }

    pub(in crate::depot) fn up<V>(
        owner: S,
        start: usize,
        value: &V,
        mut compare: impl FnMut(hole::At<&S, &V>) -> cmp::Ordering,
    ) -> Self {
        let mut end = start;
        while end != 0 {
            let parent = (end - 1) / 2;
            if !compare(hole::At::new(&owner, parent, value)).is_lt() {
                break;
            }
            end = parent;
        }
        Self { owner, start, end }
    }

    pub(in crate::depot) fn down<V>(
        owner: S,
        start: usize,
        len: usize,
        value: &V,
        mut compare: impl FnMut(hole::At<&S, &V>) -> cmp::Ordering,
    ) -> Self {
        let mut end = start;
        while len >= 2 && end <= (len - 2) / 2 {
            let left = end * 2 + 1;
            let right = left + 1;
            let child = left
                + usize::from(
                    right < len
                        && S::compare_at(
                            hole::At::new(&owner, right, ()),
                            hole::At::new(&owner, left, ()),
                        )
                        .is_lt(),
                );
            if !compare(hole::At::new(&owner, child, value)).is_gt() {
                break;
            }
            end = child;
        }
        Self { owner, start, end }
    }

    pub(in crate::depot) fn repair<V>(
        owner: S,
        start: usize,
        len: usize,
        value: &V,
        mut compare: impl FnMut(hole::At<&S, &V>) -> cmp::Ordering,
    ) -> Self {
        if start != 0 {
            let parent = (start - 1) / 2;
            if compare(hole::At::new(&owner, parent, value)).is_lt() {
                let mut path = Self::up(owner, parent, value, compare);
                path.start = start;
                return path;
            }
        }
        Self::down(owner, start, len, value, compare)
    }

    pub(in crate::depot) fn append(mut self, value: S::Value) -> heap::raw::Hole<S> {
        // SAFETY: insertion planned from the admitted next position below u32::MAX.
        *unsafe { self.owner.len_mut() } = self.start as u32 + 1;
        self.apply(value)
    }

    pub(in crate::depot) fn apply(self, value: S::Value) -> heap::raw::Hole<S> {
        let Self { owner, start, end } = self;
        let mut hole = heap::raw::Hole {
            owner,
            value,
            position: start,
        };
        if end < start {
            while hole.position != end {
                let source = (hole.position - 1) / 2;
                hole::Step {
                    hole: &mut hole,
                    source,
                }
                .shift();
            }
        } else if end > start {
            let mut depth = (start + 1).leading_zeros() - (end + 1).leading_zeros();
            loop {
                depth -= 1;
                let source = ((end + 1) >> depth) - 1;
                hole::Step {
                    hole: &mut hole,
                    source,
                }
                .shift();
                if depth == 0 {
                    break;
                }
            }
        }
        hole
    }
}

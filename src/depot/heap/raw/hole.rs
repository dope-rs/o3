use std::cmp;

use crate::depot::heap::{self, hole};

/// Owns the only hole and restores its source before returning.
pub(in crate::depot) struct Hole<S: hole::raw::Source> {
    pub(in crate::depot) owner: S,
    pub(in crate::depot) value: S::Value,
    pub(in crate::depot) position: usize,
}

impl<S: hole::raw::Source> Hole<S> {
    pub(in crate::depot) fn append(vacancy: heap::Vacancy<S, S::Value>) -> Self {
        let (mut owner, value) = vacancy.into_inner();
        let position = owner.len() as usize;
        // SAFETY: the vacancy admits this next position below u32::MAX.
        *unsafe { owner.len_mut() } = position as u32 + 1;
        let mut hole = Self {
            owner,
            value,
            position,
        };
        hole.up(S::compare);
        hole
    }

    pub(in crate::depot) fn commit<const PLACE: bool>(self) -> S {
        S::write::<PLACE>(hole::Finish { hole: self })
    }

    pub(in crate::depot) fn finish(self) -> heap::Entry<S> {
        let position = self.position;
        unsafe { heap::Entry::new(self.commit::<true>(), position, ()) }
    }

    pub(in crate::depot) fn replace<const UPDATE: bool>(
        entry: heap::Entry<S, S::Value>,
    ) -> (heap::Entry<S>, S::Value) {
        let (mut owner, position, value) = entry.into_inner();
        let direction = if UPDATE {
            S::direction(hole::At::new(&owner, position, &value))
        } else {
            cmp::Ordering::Equal
        };
        let old = S::read::<false>(hole::At::new(&mut owner, position, ()));
        let mut hole = Self {
            owner,
            value,
            position,
        };
        if UPDATE {
            match direction {
                cmp::Ordering::Less => hole.up(S::compare),
                cmp::Ordering::Greater => hole.down(hole.owner.len() as usize, S::compare),
                cmp::Ordering::Equal => {}
            }
        } else {
            hole.repair(hole.owner.len() as usize, S::compare);
        }
        let entry = if !UPDATE || hole.position != position {
            hole.finish()
        } else {
            // SAFETY: the write restores this unchanged position and its exact owner.
            unsafe { heap::Entry::new(hole.commit::<false>(), position, ()) }
        };
        (entry, old)
    }

    pub(in crate::depot) fn remove(entry: heap::Entry<S>) -> heap::Vacancy<S, S::Value> {
        let (mut owner, position, ()) = entry.into_inner();
        let tail = (owner.len() - 1) as usize;
        if position == tail {
            let value = S::read::<true>(hole::At::new(&mut owner, position, ()));
            *unsafe { owner.len_mut() } = tail as u32;
            return unsafe { heap::Vacancy::new(owner, value) };
        }
        let value = S::read::<true>(hole::At::new(&mut owner, position, ()));
        let mut hole = Self {
            owner,
            value,
            position,
        };
        // The tail stays initialized outside the active tree until its final move.
        hole.repair(tail, |entry| {
            let (owner, position, _) = entry.into_inner();
            S::compare_at(
                hole::At::new(owner, tail, ()),
                hole::At::new(owner, position, ()),
            )
        });
        hole.shift(tail);
        *unsafe { hole.owner.len_mut() } = tail as u32;
        let Self { owner, value, .. } = hole;
        unsafe { heap::Vacancy::new(owner, value) }
    }

    fn shift(&mut self, source: usize) {
        S::shift(hole::Step {
            hole: &mut *self,
            source,
        });
        self.position = source;
    }

    fn up(&mut self, mut compare: impl FnMut(hole::At<&S, &S::Value>) -> cmp::Ordering) {
        while self.position != 0 {
            let parent = (self.position - 1) / 2;
            if !compare(hole::At::new(&self.owner, parent, &self.value)).is_lt() {
                break;
            }
            self.shift(parent);
        }
    }

    fn down(
        &mut self,
        len: usize,
        mut compare: impl FnMut(hole::At<&S, &S::Value>) -> cmp::Ordering,
    ) {
        while len >= 2 && self.position <= (len - 2) / 2 {
            let left = self.position * 2 + 1;
            let right = left + 1;
            let child = left
                + usize::from(
                    right < len
                        && S::compare_at(
                            hole::At::new(&self.owner, right, ()),
                            hole::At::new(&self.owner, left, ()),
                        )
                        .is_lt(),
                );
            if !compare(hole::At::new(&self.owner, child, &self.value)).is_gt() {
                break;
            }
            self.shift(child);
        }
    }

    pub(in crate::depot) fn repair(
        &mut self,
        len: usize,
        mut compare: impl FnMut(hole::At<&S, &S::Value>) -> cmp::Ordering,
    ) {
        if self.position != 0 {
            let parent = (self.position - 1) / 2;
            if compare(hole::At::new(&self.owner, parent, &self.value)).is_lt() {
                self.shift(parent);
                self.up(compare);
                return;
            }
        }
        self.down(len, compare);
    }
}

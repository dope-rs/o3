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
        let (owner, value) = vacancy.into_inner();
        let position = owner.len() as usize;
        hole::Path::up(owner, position, &value, S::compare).append(value)
    }

    pub(in crate::depot) fn prepare(entry: heap::Entry<S, S::Value>) -> heap::Entry<S> {
        let (entry, old) = Self::replace(entry);
        drop(old);
        entry
    }

    pub(in crate::depot) fn commit<const PLACE: bool>(self) -> S {
        S::write::<PLACE>(hole::Finish { hole: self })
    }

    pub(in crate::depot) fn finish(self) -> heap::Entry<S> {
        let position = self.position;
        unsafe { heap::Entry::new(self.commit::<true>(), position, ()) }
    }

    pub(in crate::depot) fn replace(entry: heap::Entry<S, S::Value>) -> (heap::Entry<S>, S::Value) {
        let (owner, position, value) = entry.into_inner();
        let len = owner.len() as usize;
        let mut path = hole::Path::repair(owner, position, len, &value, S::compare);
        let old = S::read::<false>(hole::At::new(&mut path.owner, position, ()));
        (path.apply(value).finish(), old)
    }

    pub(in crate::depot) fn update(entry: heap::Entry<S, S::Value>) {
        let (owner, position, value) = entry.into_inner();
        let order = S::direction(hole::At::new(&owner, position, &value));
        let mut path = match order {
            cmp::Ordering::Less => hole::Path::up(owner, position, &value, S::compare),
            cmp::Ordering::Greater => {
                let len = owner.len() as usize;
                hole::Path::down(owner, position, len, &value, S::compare)
            }
            cmp::Ordering::Equal => hole::Path::at(owner, position),
        };
        let old = S::read::<false>(hole::At::new(&mut path.owner, position, ()));
        let hole = path.apply(value);
        if hole.position != position {
            hole.commit::<true>();
        } else {
            hole.commit::<false>();
        }
        drop(old);
    }

    pub(in crate::depot) fn remove(entry: heap::Entry<S>) -> heap::Vacancy<S, S::Value> {
        let (mut owner, position, ()) = entry.into_inner();
        let tail = (owner.len() - 1) as usize;
        if position == tail {
            let value = S::read::<true>(hole::At::new(&mut owner, position, ()));
            *unsafe { owner.len_mut() } = tail as u32;
            return unsafe { heap::Vacancy::new(owner, value) };
        }
        let mut path = hole::Path::repair(owner, position, tail, &tail, |entry| {
            let (owner, position, tail) = entry.into_inner();
            S::compare_at(
                hole::At::new(owner, *tail, ()),
                hole::At::new(owner, position, ()),
            )
        });
        let value = S::read::<true>(hole::At::new(&mut path.owner, position, ()));
        let mut hole = path.apply(value);
        hole::Step {
            hole: &mut hole,
            source: tail,
        }
        .shift();
        *unsafe { hole.owner.len_mut() } = tail as u32;
        let Self { owner, value, .. } = hole;
        unsafe { heap::Vacancy::new(owner, value) }
    }

    pub(in crate::depot) fn take(entry: heap::Entry<S>) -> S::Value {
        Self::remove(entry).into_inner().1
    }

    pub(in crate::depot) fn retire(entry: heap::Entry<S>) -> S {
        entry.remove().0
    }
}

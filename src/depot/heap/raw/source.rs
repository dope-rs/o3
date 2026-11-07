use std::{cmp, ptr};

use crate::depot::{
    self,
    heap::{self, hole},
    raw::{Link as _, Word as _},
};

unsafe impl<
    'a,
    K: depot::raw::Address + 'a,
    P: Ord + 'a,
    S: heap::raw::Store<Key = K, Priority = P>,
> hole::raw::Source for &'a mut S
{
    type Value = heap::Stored<K, P>;

    fn len(&self) -> u32 {
        self.storage().state
    }

    unsafe fn len_mut(&mut self) -> &mut u32 {
        &mut self.storage_mut().state
    }

    fn compare(entry: hole::At<&Self, &Self::Value>) -> cmp::Ordering {
        let (owner, position, value) = entry.into_inner();
        let other = unsafe { owner.values().get_unchecked(position).assume_init_ref() };
        compare::<S>(value, other)
    }

    fn compare_at(left: hole::At<&Self>, right: hole::At<&Self>) -> cmp::Ordering {
        let (first, left, ()) = left.into_inner();
        let (second, right, ()) = right.into_inner();
        let left = unsafe { first.values().get_unchecked(left).assume_init_ref() };
        let right = unsafe { second.values().get_unchecked(right).assume_init_ref() };
        compare::<S>(left, right)
    }

    fn read<const REMOVE: bool>(entry: hole::At<&mut Self>) -> Self::Value {
        let (owner, position, ()) = entry.into_inner();
        let (_, entries, map) = owner.storage_mut().parts_mut();
        // SAFETY: At retains this exact initialized heap position and its exclusive owner.
        let slot = unsafe { entries.get_unchecked_mut(position) };
        if REMOVE {
            // SAFETY: the same owner retains the slot's reciprocal reverse-map entry.
            unsafe { S::reindex(map, slot.assume_init_ref().identity, S::Position::NONE) };
        }
        // SAFETY: detachment retains this exact initialized slot until it is moved once.
        unsafe { slot.assume_init_read() }
    }

    fn shift(step: hole::Step<'_, Self>) {
        let (owner, source, target) = step.into_inner();
        let (_, entries, map) = owner.storage_mut().parts_mut();
        let position = target;
        let [target, source] = unsafe { entries.get_disjoint_unchecked_mut([target, source]) };
        let value = unsafe { source.assume_init_ref() };
        // SAFETY: Step retains distinct positions and this owner's exact reverse map.
        unsafe {
            S::reindex(map, value.identity, S::Position::from_offset(position));
            ptr::copy_nonoverlapping(value, target.as_mut_ptr(), 1);
        }
    }

    fn write<const PLACE: bool>(finish: hole::Finish<Self>) -> Self {
        let (owner, position, value) = finish.into_inner();
        let (_, entries, map) = owner.storage_mut().parts_mut();
        let target = unsafe { entries.get_unchecked_mut(position) };
        if PLACE {
            // SAFETY: Finish retains this key and its admitted destination in the same owner.
            unsafe { S::reindex(map, value.identity, S::Position::from_offset(position)) };
        }
        target.write(value);
        owner
    }
}

fn compare<S: heap::raw::Store>(
    left: &heap::Stored<S::Key, S::Priority>,
    right: &heap::Stored<S::Key, S::Priority>,
) -> cmp::Ordering {
    let left = (left.identity, &left.priority);
    let right = (right.identity, &right.priority);
    match left.1.cmp(right.1) {
        cmp::Ordering::Equal if S::precedes(&left.0, &right.0) => cmp::Ordering::Less,
        cmp::Ordering::Equal if S::precedes(&right.0, &left.0) => cmp::Ordering::Greater,
        order => order,
    }
}

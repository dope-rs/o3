use std::{cell::Cell, cmp::Ordering, mem::MaybeUninit};

use o3::depot::heap::hole::{self, At, Finish, Step};

use crate::common::Counted;

pub struct Priority<'a> {
    pub value: u32,
    pub drops: Counted<&'a Cell<Option<usize>>>,
}

pub struct Split<'a> {
    pub len: u32,
    pub slots: [usize; 7],
    pub priorities: [MaybeUninit<Priority<'a>>; 7],
    pub positions: [Option<usize>; 7],
    pub writes: Option<usize>,
}

impl Default for Split<'_> {
    fn default() -> Self {
        Self {
            len: 0,
            slots: [0; 7],
            priorities: [const { MaybeUninit::uninit() }; 7],
            positions: [None; 7],
            writes: Some(0),
        }
    }
}

unsafe impl<'a> hole::raw::Source for &mut Split<'a> {
    type Value = (usize, Priority<'a>);

    fn len(&self) -> u32 {
        self.len
    }

    unsafe fn len_mut(&mut self) -> &mut u32 {
        &mut self.len
    }

    fn compare(entry: At<&Self, &Self::Value>) -> Ordering {
        let (owner, position, (slot, priority)) = entry.into_inner();
        priority
            .value
            .cmp(&unsafe { owner.priorities.get_unchecked(position).assume_init_ref() }.value)
            .then_with(|| slot.cmp(&owner.slots[position]))
    }

    fn compare_at(left: At<&Self>, right: At<&Self>) -> Ordering {
        let (owner, left, ()) = left.into_inner();
        let (other, right, ()) = right.into_inner();
        let first = unsafe { owner.priorities.get_unchecked(left).assume_init_ref() };
        let second = unsafe { other.priorities.get_unchecked(right).assume_init_ref() };
        (first.value, owner.slots[left]).cmp(&(second.value, other.slots[right]))
    }

    fn read<const REMOVE: bool>(entry: At<&mut Self>) -> Self::Value {
        let (owner, position, ()) = entry.into_inner();
        let slot = owner.slots[position];
        if REMOVE {
            owner.positions[slot] = None;
        }
        (slot, unsafe {
            owner.priorities.get_unchecked(position).assume_init_read()
        })
    }

    fn shift(step: Step<'_, Self>) {
        let (owner, source, target) = step.into_inner();
        let slot = owner.slots[source];
        let priority = unsafe { owner.priorities.get_unchecked(source).assume_init_read() };
        owner.slots[target] = slot;
        owner.priorities[target].write(priority);
        owner.positions[slot] = Some(target);
        owner.writes = owner.writes.and_then(|count| count.checked_add(1));
    }

    fn write<const PLACE: bool>(finish: Finish<Self>) -> Self {
        let (owner, position, (slot, priority)) = finish.into_inner();
        owner.priorities[position].write(priority);
        if PLACE {
            owner.slots[position] = slot;
            owner.positions[slot] = Some(position);
            owner.writes = owner.writes.and_then(|count| count.checked_add(1));
        }
        owner
    }
}

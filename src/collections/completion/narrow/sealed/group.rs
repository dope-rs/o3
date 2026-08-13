use std::{cell, mem, ptr};

use crate::collections;

const OCCUPIED: usize = 1;

pub(super) struct Entry<T: Copy> {
    pub(super) value: cell::Cell<mem::MaybeUninit<T>>,
    pub(super) state: cell::Cell<*mut ()>,
    pub(super) index: u32,
    pub(super) generation: cell::Cell<u32>,
}

pub(super) struct Group<T: Copy> {
    pub(super) entries: Box<[Entry<T>]>,
    pub(super) free: cell::Cell<*mut Entry<T>>,
    pub(super) leased: cell::Cell<bool>,
    pub(super) next: cell::Cell<Option<ptr::NonNull<Group<T>>>>,
}

impl<T: Copy> Group<T> {
    pub(super) fn try_new(
        capacity: usize,
        base: u32,
    ) -> Result<ptr::NonNull<Self>, collections::AllocationError> {
        let entries = collections::BoxSliceExt::try_box_with(capacity, |offset| Entry {
            value: cell::Cell::new(mem::MaybeUninit::uninit()),
            state: cell::Cell::new(ptr::null_mut()),
            index: base + offset as u32,
            generation: cell::Cell::new(0),
        })?;
        let group = collections::BoxExt::try_box(Self {
            entries,
            free: cell::Cell::new(ptr::null_mut()),
            leased: cell::Cell::new(false),
            next: cell::Cell::new(None),
        })?;
        Ok(unsafe { ptr::NonNull::new_unchecked(Box::into_raw(group)) })
    }

    pub(super) fn pop(list: &mut Option<ptr::NonNull<Self>>) -> Option<ptr::NonNull<Self>> {
        let group = list.take()?;
        *list = unsafe { group.as_ref() }.next.replace(None);
        Some(group)
    }

    pub(super) fn push(list: &mut Option<ptr::NonNull<Self>>, group: ptr::NonNull<Self>) {
        let group_ref = unsafe { group.as_ref() };
        debug_assert!(group_ref.next.get().is_none());
        group_ref.next.set(list.take());
        *list = Some(group);
    }

    pub(super) unsafe fn drop_owned(group: ptr::NonNull<Self>) {
        debug_assert!(unsafe { group.as_ref() }.next.get().is_none());
        drop(unsafe { Box::from_raw(group.as_ptr()) });
    }

    pub(super) fn is_quiescent(&self) -> bool {
        !self.leased.get()
            && self
                .entries
                .iter()
                .all(|entry| entry.state.get().addr() & OCCUPIED == 0)
    }

    pub(super) fn available(&self, generation_max: u32) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.generation.get() < generation_max)
            .count()
    }

    pub(super) fn prepare(&self, capacity: usize, generation_max: u32) {
        debug_assert!(!self.leased.get());
        debug_assert!(capacity <= self.available(generation_max));
        let mut remaining = capacity;
        let mut next: *mut Entry<T> = ptr::null_mut();
        for entry in self.entries.iter().rev() {
            debug_assert_eq!(entry.state.get().addr() & OCCUPIED, 0);
            if remaining != 0 && entry.generation.get() < generation_max {
                entry.state.set(next.cast());
                next = ptr::from_ref(entry).cast_mut();
                remaining -= 1;
            }
        }
        debug_assert_eq!(remaining, 0);
        self.free.set(next);
        self.leased.set(true);
    }
}

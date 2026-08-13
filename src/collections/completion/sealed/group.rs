use std::{cell, mem, ptr};

use crate::collections;

const OCCUPIED: usize = 1;

pub(super) struct Entry<T: Copy> {
    pub(super) value: cell::Cell<mem::MaybeUninit<T>>,
    pub(super) state: cell::Cell<*mut ()>,
    pub(super) serial: cell::Cell<u64>,
}

pub(super) struct Group<T: Copy> {
    pub(super) entries: Box<[Entry<T>]>,
    pub(super) free: cell::Cell<*mut Entry<T>>,
    pub(super) leased: cell::Cell<bool>,
    pub(super) next: cell::Cell<Option<ptr::NonNull<Group<T>>>>,
    pub(super) serial: cell::Cell<u64>,
}

impl<T: Copy> Group<T> {
    pub(super) fn try_new(
        capacity: usize,
    ) -> Result<ptr::NonNull<Self>, collections::AllocationError> {
        let entries = collections::BoxSliceExt::try_box_with(capacity, |_| Entry {
            value: cell::Cell::new(mem::MaybeUninit::uninit()),
            state: cell::Cell::new(ptr::null_mut()),
            serial: cell::Cell::new(0),
        })?;
        let group = collections::BoxExt::try_box(Self {
            entries,
            free: cell::Cell::new(ptr::null_mut()),
            leased: cell::Cell::new(false),
            next: cell::Cell::new(None),
            serial: cell::Cell::new(0),
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

    pub(super) fn prepare(&self, capacity: usize) {
        debug_assert!(!self.leased.get());
        debug_assert!(capacity <= self.entries.len());
        let mut next: *mut Entry<T> = ptr::null_mut();
        for entry in self.entries[..capacity].iter().rev() {
            debug_assert_eq!(entry.state.get().addr() & OCCUPIED, 0);
            entry.state.set(next.cast());
            next = ptr::from_ref(entry).cast_mut();
        }
        self.free.set(next);
        self.leased.set(true);
    }
}

//! Observes the layouts and fallible allocation paths implemented by o3.

use std::{
    alloc::{GlobalAlloc, Layout, System},
    array,
    cell::Cell,
    ptr,
};

use o3::base::marker::ThreadBound;

thread_local! {
    static COUNTS: Cell<[usize; 4]> = const { Cell::new([0; 4]) };
    static REALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static FAIL: Cell<Option<usize>> = const { Cell::new(None) };
}

pub struct Tracker;

#[derive(Debug, Default, PartialEq)]
pub struct Activity {
    pub allocations: usize,
    pub allocated_bytes: usize,
    pub deallocations: usize,
    pub deallocated_bytes: usize,
}

pub struct Measurement([usize; 4], ThreadBound);

impl Measurement {
    pub fn finish(self) -> Activity {
        let [a, ab, d, db] = self.delta();
        Activity {
            allocations: a,
            allocated_bytes: ab,
            deallocations: d,
            deallocated_bytes: db,
        }
    }

    fn delta(self) -> [usize; 4] {
        COUNTS
            .with(|counts| array::from_fn(|index| counts.get()[index].wrapping_sub(self.0[index])))
    }
}

impl Tracker {
    pub fn begin() -> Measurement {
        Measurement(COUNTS.with(Cell::get), ThreadBound::NEW)
    }

    pub fn measure<T>(run: impl FnOnce() -> T) -> (T, [usize; 4]) {
        let before = Self::begin();
        (run(), before.delta())
    }

    pub fn activity<T>(run: impl FnOnce() -> T) -> (T, Activity) {
        let before = Self::begin();
        (run(), before.finish())
    }

    pub fn count<T>(run: impl FnOnce() -> T) -> (T, usize) {
        let (value, counts) = Self::measure(run);
        (value, counts[0])
    }

    pub fn counts<T>(run: impl FnOnce() -> T) -> (T, [usize; 2]) {
        let (value, counts) = Self::measure(run);
        (value, [counts[0], counts[2]])
    }

    pub fn usage<T>(run: impl FnOnce() -> T) -> (T, (usize, usize)) {
        let (value, counts) = Self::measure(run);
        (value, (counts[0], counts[1]))
    }

    pub fn resizes<T>(run: impl FnOnce() -> T) -> (T, usize) {
        let before = REALLOCATIONS.with(Cell::get);
        let value = run();
        let count = REALLOCATIONS.with(Cell::get).wrapping_sub(before);
        (value, count)
    }

    pub fn fail_after<T>(allowed: usize, run: impl FnOnce() -> T) -> T {
        let previous = FAIL.with(|fail| fail.replace(Some(allowed)));
        let value = run();
        FAIL.with(|fail| fail.set(previous));
        value
    }

    fn is_rejected() -> bool {
        FAIL.with(|fail| match fail.get() {
            Some(0) => {
                fail.set(None);
                true
            }
            Some(left) => {
                fail.set(Some(left - 1));
                false
            }
            None => false,
        })
    }

    fn record(new: Option<usize>, old: Option<usize>) {
        COUNTS.with(|counts| {
            let mut values = counts.get();
            for (offset, bytes) in [(0, new), (2, old)] {
                if let Some(bytes) = bytes {
                    values[offset] = values[offset].wrapping_add(1);
                    values[offset + 1] = values[offset + 1].wrapping_add(bytes);
                }
            }
            counts.set(values);
        });
    }
}

// SAFETY: requests and pointers are forwarded unchanged to System. Thread-local
// observation never touches allocated memory.
unsafe impl GlobalAlloc for Tracker {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if Self::is_rejected() {
            return ptr::null_mut();
        }
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            Self::record(Some(layout.size()), None);
        }
        pointer
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if Self::is_rejected() {
            return ptr::null_mut();
        }
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            Self::record(Some(layout.size()), None);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        Self::record(None, Some(layout.size()));
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if Self::is_rejected() {
            return ptr::null_mut();
        }
        let resized = unsafe { System.realloc(pointer, layout, size) };
        if !resized.is_null() {
            REALLOCATIONS.with(|count| count.set(count.get().wrapping_add(1)));
            Self::record(Some(size), Some(layout.size()));
        }
        resized
    }
}

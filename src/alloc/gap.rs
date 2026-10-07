use std::ptr;

use crate::base::marker::ThreadBound;

/// A searched insertion position retaining its original vector exclusively.
#[derive(Debug)]
#[must_use = "insert a value or abandon the searched position"]
pub struct Gap<'a, T> {
    values: &'a mut Vec<T>,
    index: usize,
    _bound: ThreadBound,
}

impl<'a, T> Gap<'a, T> {
    /// Retains the partition point and owner from one search of the current slice.
    pub fn partition(values: &'a mut Vec<T>, before: impl FnMut(&T) -> bool) -> Self {
        let index = values.partition_point(before);
        Self {
            values,
            index,
            _bound: ThreadBound::NEW,
        }
    }

    pub const fn index(&self) -> usize {
        self.index
    }

    pub fn tail(&self) -> &[T] {
        // SAFETY: partition_point returned a position at most this vector's
        // length, and the exclusive loan preserves its allocation and length.
        unsafe { self.values.get_unchecked(self.index..) }
    }

    /// Inserts without allocating, returning the value unchanged when full.
    pub fn try_insert(self, value: T) -> Result<(), T> {
        let len = self.values.len();
        if len == self.values.capacity() {
            return Err(value);
        }
        // SAFETY: the retained search bounds index by len. The capacity check
        // admits len + 1, including zero-sized elements. Moving the suffix,
        // writing the gap and publishing the length preserve each value once;
        // no user code runs between these operations.
        unsafe {
            let start = self.values.as_mut_ptr().add(self.index);
            ptr::copy(start, start.add(1), len - self.index);
            start.write(value);
            self.values.set_len(len + 1);
        }
        Ok(())
    }
}

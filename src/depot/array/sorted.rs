//! Fixed-capacity sets that retain their ordering through every insertion.

use std::{mem, ptr};

use crate::depot::array::prefix::Prefix;

/// An ordered inline set with no allocation or drop glue.
#[repr(transparent)]
pub struct Set<T: Copy, const N: usize> {
    prefix: Prefix<T, N>,
}

/// A rejected set member and the exact observed conflict or capacity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error<T> {
    Duplicate { value: T, existing: T },
    Full { value: T, capacity: usize },
}

impl<T: Copy + Ord, const N: usize> Default for Set<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + Ord, const N: usize> Set<T, N> {
    pub const fn new() -> Self {
        Self {
            prefix: Prefix::empty(),
        }
    }

    /// Searches once and consumes the resulting position in this same owner.
    pub fn try_insert(&mut self, value: T) -> Result<(), Error<T>> {
        let values = self.prefix.as_slice();
        let index = match values.binary_search(&value) {
            Ok(index) => {
                // SAFETY: binary_search selected an element of this exact slice.
                let existing = unsafe { *values.get_unchecked(index) };
                return Err(Error::Duplicate { value, existing });
            }
            Err(index) => index,
        };
        let next = self
            .prefix
            .len
            .vacancy()
            .ok_or(Error::Full { value, capacity: N })?;
        // SAFETY: the search bounds index by the current initialized length;
        // next proves room for the shifted suffix in this exclusively borrowed owner.
        unsafe {
            let start = self.prefix.entries.as_mut_ptr().add(index);
            ptr::copy(start, start.add(1), self.prefix.len.get() - index);
            start.write(mem::MaybeUninit::new(value));
        }
        self.prefix.len = next.commit();
        Ok(())
    }
}

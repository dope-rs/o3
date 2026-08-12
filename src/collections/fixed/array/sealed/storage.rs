use std::{array, mem, slice};

#[repr(C)]
pub(super) struct Storage<T, const N: usize> {
    entries: [mem::MaybeUninit<T>; N],
    len: u32,
}

impl<T: Copy, const N: usize> Copy for Storage<T, N> {}

impl<T: Copy, const N: usize> Clone for Storage<T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T, const N: usize> Storage<T, N> {
    const VALID: () = assert!(N <= u32::MAX as usize, "inline array capacity must fit u32");

    pub(super) fn new() -> Self {
        let () = Self::VALID;
        Self {
            entries: array::from_fn(|_| mem::MaybeUninit::uninit()),
            len: 0,
        }
    }

    pub(super) fn from_fn(len: usize, mut f: impl FnMut(usize) -> T) -> Self {
        assert!(len <= N, "inline array length exceeds capacity");
        let mut storage = Self::new();
        for index in 0..len {
            storage.entries[index].write(f(index));
            storage.len = (index + 1) as u32;
        }
        storage
    }

    pub(super) fn push(&mut self, value: T) -> Result<(), T> {
        let len = self.len();
        if len == N {
            return Err(value);
        }
        self.entries[len].write(value);
        self.len += 1;
        Ok(())
    }

    pub(super) fn insert(&mut self, index: usize, value: T) -> Result<(), T>
    where
        T: Copy,
    {
        let len = self.len();
        if index > len || len == N {
            return Err(value);
        }
        self.entries.copy_within(index..len, index + 1);
        self.entries[index].write(value);
        self.len += 1;
        Ok(())
    }

    pub(super) fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: the previous last entry was initialized, and reducing `len`
        // transfers its drop obligation to the returned value.
        Some(unsafe { self.entries[self.len()].assume_init_read() })
    }

    pub(super) fn clear_copy(&mut self) {
        self.len = 0;
    }

    pub(super) fn truncate_copy(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    pub(super) fn try_extend_from_slice<'a>(&mut self, values: &'a [T]) -> Result<(), &'a [T]>
    where
        T: Copy,
    {
        let start = self.len();
        let Some(end) = start.checked_add(values.len()) else {
            return Err(values);
        };
        if end > N {
            return Err(values);
        }
        for (entry, value) in self.entries[start..end].iter_mut().zip(values) {
            entry.write(*value);
        }
        self.len = end as u32;
        Ok(())
    }

    pub(super) fn len(&self) -> usize {
        self.len as usize
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn is_full(&self) -> bool {
        self.len() == N
    }

    pub(super) fn as_slice(&self) -> &[T] {
        // SAFETY: `0..len` is the initialized prefix and `MaybeUninit<T>` has
        // the same layout and alignment as `T`.
        unsafe { slice::from_raw_parts(self.entries.as_ptr().cast(), self.len()) }
    }

    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: `0..len` is initialized and uniquely borrowed through
        // `&mut self`; mutation cannot change which entries are initialized.
        unsafe { slice::from_raw_parts_mut(self.entries.as_mut_ptr().cast(), self.len()) }
    }

    pub(super) fn into_iter(self) -> super::IntoIter<T, N> {
        let len = self.len();
        super::IntoIter {
            entries: self.entries,
            index: 0,
            len,
        }
    }

    pub(super) fn truncate(&mut self, len: usize) {
        let current = self.len();
        if len >= current {
            return;
        }
        self.len = len as u32;
        drop(super::DropEntries {
            entries: &mut self.entries[len..current],
        });
    }
}

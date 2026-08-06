use std::{
    array::from_fn,
    iter::FusedIterator,
    mem::{ManuallyDrop, MaybeUninit, forget, take},
    ptr::{addr_of, read},
    slice::from_raw_parts,
};

/// An inline vector with a fixed capacity.
///
/// The prefix `0..len` is initialized and owns its values.
#[repr(transparent)]
pub struct Inline<T, const N: usize> {
    storage: Storage<T, N>,
}

/// An inline fixed-capacity vector for [`Copy`] values.
#[repr(transparent)]
pub struct CopyInline<T: Copy, const N: usize> {
    storage: Storage<T, N>,
}

#[repr(C)]
struct Storage<T, const N: usize> {
    entries: [MaybeUninit<T>; N],
    len: usize,
}

/// A consuming iterator over an [`Inline`].
#[repr(C)]
pub struct IntoIter<T, const N: usize> {
    entries: [MaybeUninit<T>; N],
    index: usize,
    len: usize,
}

impl<T, const N: usize> Inline<T, N> {
    fn new() -> Self {
        Self {
            storage: Storage::new(),
        }
    }

    pub fn from_fn(len: usize, mut f: impl FnMut(usize) -> T) -> Self {
        let mut values = Self::new();
        values.storage.fill_with(len, &mut f);
        values
    }
}

impl<T: Copy, const N: usize> CopyInline<T, N> {
    pub fn new() -> Self {
        Self {
            storage: Storage::new(),
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        self.storage.push(value)
    }

    pub fn len(&self) -> usize {
        self.storage.len
    }

    pub fn is_empty(&self) -> bool {
        self.storage.len == 0
    }

    pub fn is_full(&self) -> bool {
        self.storage.len == N
    }

    pub fn as_slice(&self) -> &[T] {
        self.storage.as_slice()
    }
}

impl<T: Copy, const N: usize> Default for CopyInline<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> Storage<T, N> {
    fn new() -> Self {
        Self {
            entries: from_fn(|_| MaybeUninit::uninit()),
            len: 0,
        }
    }

    fn fill_with(&mut self, len: usize, f: &mut impl FnMut(usize) -> T) {
        assert!(len <= N, "array vector length exceeds capacity");
        for (index, entry) in self.entries[..len].iter_mut().enumerate() {
            entry.write(f(index));
            self.len += 1;
        }
    }

    fn push(&mut self, value: T) -> Result<(), T> {
        if self.len == N {
            return Err(value);
        }
        self.entries[self.len].write(value);
        self.len += 1;
        Ok(())
    }

    fn as_slice(&self) -> &[T] {
        // SAFETY: `0..len` is the initialized prefix and `MaybeUninit<T>` has
        // the same layout and alignment as `T`.
        unsafe { from_raw_parts(self.entries.as_ptr().cast(), self.len) }
    }
}

impl<T, const N: usize> IntoIterator for Inline<T, N> {
    type IntoIter = IntoIter<T, N>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        let value = ManuallyDrop::new(self);
        let source = &value as *const ManuallyDrop<Self> as *const Self;
        // SAFETY: `value` suppresses Inline's Drop. Moving the backing array
        // transfers its initialized prefix to the returned iterator exactly once.
        let entries = unsafe { read(addr_of!((*source).storage.entries)) };
        let len = unsafe { (*source).storage.len };
        IntoIter {
            entries,
            index: 0,
            len,
        }
    }
}

impl<T, const N: usize> Iterator for IntoIter<T, N> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.index == self.len {
            return None;
        }
        let index = self.index;
        self.index += 1;
        // SAFETY: `index` names an initialized, unread entry. Advancing first
        // transfers its drop obligation to the returned value.
        Some(unsafe { self.entries[index].assume_init_read() })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.len - self.index;
        (remaining, Some(remaining))
    }
}

impl<T, const N: usize> DoubleEndedIterator for IntoIter<T, N> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index == self.len {
            return None;
        }
        self.len -= 1;
        // SAFETY: `len` now names the last initialized, unread entry.
        Some(unsafe { self.entries[self.len].assume_init_read() })
    }
}

impl<T, const N: usize> ExactSizeIterator for IntoIter<T, N> {}
impl<T, const N: usize> FusedIterator for IntoIter<T, N> {}

impl<T, const N: usize> Drop for Inline<T, N> {
    fn drop(&mut self) {
        let len = take(&mut self.storage.len);
        drop(DropEntries {
            entries: &mut self.storage.entries[..len],
        });
    }
}

impl<T, const N: usize> Drop for IntoIter<T, N> {
    fn drop(&mut self) {
        let index = take(&mut self.index);
        drop(DropEntries {
            entries: &mut self.entries[index..self.len],
        });
    }
}

struct DropEntries<'a, T> {
    entries: &'a mut [MaybeUninit<T>],
}

impl<T> Drop for DropEntries<'_, T> {
    fn drop(&mut self) {
        loop {
            let entries = take(&mut self.entries);
            let Some((head, tail)) = entries.split_first_mut() else {
                break;
            };
            let mut remaining = Self { entries: tail };
            // SAFETY: this guard owns exactly the initialized suffix. On an
            // unwind, `remaining` drops the tail before propagating it.
            unsafe { head.assume_init_drop() };
            self.entries = take(&mut remaining.entries);
            forget(remaining);
        }
    }
}

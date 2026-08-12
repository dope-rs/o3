use std::{fmt, iter, mem, ops, ptr, slice};

mod storage;

/// An inline vector with a fixed capacity for values that require destruction.
///
/// The initialized prefix owns its values and is released exactly once.
#[repr(transparent)]
pub struct Inline<T, const N: usize> {
    storage: storage::Storage<T, N>,
}

/// An inline vector with a fixed capacity for [`Copy`] values.
///
/// Unlike [`Inline`], this type has no drop glue.
#[repr(transparent)]
#[derive(Clone, Copy)]
pub struct CopyInline<T: Copy, const N: usize> {
    storage: storage::Storage<T, N>,
}

/// A consuming iterator over an [`Inline`] or [`CopyInline`].
#[repr(C)]
pub struct IntoIter<T, const N: usize> {
    entries: [mem::MaybeUninit<T>; N],
    index: usize,
    len: usize,
}

impl<T, const N: usize> Inline<T, N> {
    pub fn new() -> Self {
        Self {
            storage: storage::Storage::new(),
        }
    }

    pub fn from_fn(len: usize, f: impl FnMut(usize) -> T) -> Self {
        Self {
            storage: storage::Storage::from_fn(len, f),
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        self.storage.push(value)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.storage.pop()
    }

    pub fn clear(&mut self) {
        self.storage.truncate(0);
    }

    pub fn truncate(&mut self, len: usize) {
        self.storage.truncate(len);
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.storage.is_full()
    }

    pub fn as_slice(&self) -> &[T] {
        self.storage.as_slice()
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.storage.as_mut_slice()
    }
}

impl<T: Copy, const N: usize> Inline<T, N> {
    pub fn try_extend_from_slice<'a>(&mut self, values: &'a [T]) -> Result<(), &'a [T]> {
        self.storage.try_extend_from_slice(values)
    }

    pub fn try_from_slice(values: &[T]) -> Result<Self, &[T]> {
        let mut inline = Self::new();
        inline.try_extend_from_slice(values)?;
        Ok(inline)
    }
}

impl<T, const N: usize> Default for Inline<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone, const N: usize> Clone for Inline<T, N> {
    fn clone(&self) -> Self {
        Self::from_fn(self.len(), |index| self[index].clone())
    }
}

impl<T: fmt::Debug, const N: usize> fmt::Debug for Inline<T, N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.as_slice()).finish()
    }
}

impl<T: PartialEq, const N: usize> PartialEq for Inline<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Eq, const N: usize> Eq for Inline<T, N> {}

impl<T, const N: usize> AsRef<[T]> for Inline<T, N> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T, const N: usize> ops::Deref for Inline<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T, const N: usize> ops::DerefMut for Inline<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T, const N: usize> IntoIterator for Inline<T, N> {
    type IntoIter = IntoIter<T, N>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        let value = mem::ManuallyDrop::new(self);
        // SAFETY: `value` suppresses `Inline::drop`; moving its storage
        // transfers the initialized prefix and exact drop obligation.
        let storage = unsafe { ptr::read(&value.storage) };
        storage.into_iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a Inline<T, N> {
    type IntoIter = slice::Iter<'a, T>;
    type Item = &'a T;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T, const N: usize> IntoIterator for &'a mut Inline<T, N> {
    type IntoIter = slice::IterMut<'a, T>;
    type Item = &'a mut T;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
    }
}

impl<T, const N: usize> Drop for Inline<T, N> {
    fn drop(&mut self) {
        self.clear();
    }
}

impl<T: Copy, const N: usize> CopyInline<T, N> {
    pub fn new() -> Self {
        Self {
            storage: storage::Storage::new(),
        }
    }

    pub fn from_fn(len: usize, f: impl FnMut(usize) -> T) -> Self {
        Self {
            storage: storage::Storage::from_fn(len, f),
        }
    }

    pub fn from_array(values: [T; N]) -> Self {
        Self::from_fn(N, |index| values[index])
    }

    pub fn try_from_slice(values: &[T]) -> Result<Self, &[T]> {
        let mut inline = Self::new();
        inline.try_extend_from_slice(values)?;
        Ok(inline)
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        self.storage.push(value)
    }

    pub fn insert(&mut self, index: usize, value: T) -> Result<(), T> {
        self.storage.insert(index, value)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.storage.pop()
    }

    pub fn clear(&mut self) {
        self.storage.clear_copy();
    }

    pub fn truncate(&mut self, len: usize) {
        self.storage.truncate_copy(len);
    }

    pub fn try_extend_from_slice<'a>(&mut self, values: &'a [T]) -> Result<(), &'a [T]> {
        self.storage.try_extend_from_slice(values)
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.storage.is_full()
    }

    pub fn as_slice(&self) -> &[T] {
        self.storage.as_slice()
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.storage.as_mut_slice()
    }
}

impl<T: Copy, const N: usize> Default for CopyInline<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy + fmt::Debug, const N: usize> fmt::Debug for CopyInline<T, N> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.as_slice()).finish()
    }
}

impl<T: Copy + PartialEq, const N: usize> PartialEq for CopyInline<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl<T: Copy + Eq, const N: usize> Eq for CopyInline<T, N> {}

impl<T: Copy, const N: usize> AsRef<[T]> for CopyInline<T, N> {
    fn as_ref(&self) -> &[T] {
        self.as_slice()
    }
}

impl<T: Copy, const N: usize> ops::Deref for CopyInline<T, N> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: Copy, const N: usize> ops::DerefMut for CopyInline<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T: Copy, const N: usize> IntoIterator for CopyInline<T, N> {
    type IntoIter = IntoIter<T, N>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        self.storage.into_iter()
    }
}

impl<'a, T: Copy, const N: usize> IntoIterator for &'a CopyInline<T, N> {
    type IntoIter = slice::Iter<'a, T>;
    type Item = &'a T;

    fn into_iter(self) -> Self::IntoIter {
        self.as_slice().iter()
    }
}

impl<'a, T: Copy, const N: usize> IntoIterator for &'a mut CopyInline<T, N> {
    type IntoIter = slice::IterMut<'a, T>;
    type Item = &'a mut T;

    fn into_iter(self) -> Self::IntoIter {
        self.as_mut_slice().iter_mut()
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
impl<T, const N: usize> iter::FusedIterator for IntoIter<T, N> {}

impl<T, const N: usize> Drop for IntoIter<T, N> {
    fn drop(&mut self) {
        let index = mem::replace(&mut self.index, self.len);
        drop(DropEntries {
            entries: &mut self.entries[index..self.len],
        });
    }
}

struct DropEntries<'a, T> {
    entries: &'a mut [mem::MaybeUninit<T>],
}

impl<T> Drop for DropEntries<'_, T> {
    fn drop(&mut self) {
        loop {
            let entries = mem::take(&mut self.entries);
            let Some((head, tail)) = entries.split_first_mut() else {
                break;
            };
            let mut remaining = Self { entries: tail };
            // SAFETY: this guard owns exactly the initialized suffix. On an
            // unwind, `remaining` drops the tail before propagating it.
            unsafe { head.assume_init_drop() };
            self.entries = mem::take(&mut remaining.entries);
            mem::forget(remaining);
        }
    }
}

use std::array::from_fn;
use std::iter::FusedIterator;
use std::mem::{ManuallyDrop, MaybeUninit, forget, take};

/// An inline vector with a fixed capacity.
///
/// The prefix `0..len` is initialized and owns its values.
#[repr(transparent)]
pub struct ArrayVec<T, const N: usize> {
    storage: ArrayStorage<T, N>,
}

/// An inline vector for copyable values with no destructor.
///
/// The prefix `0..len` is initialized. Restricting values to [`Copy`] keeps
/// containers embedding this vector trivially movable and droppable.
#[repr(transparent)]
pub struct CopyArrayVec<T: Copy, const N: usize> {
    storage: ArrayStorage<T, N>,
}

#[repr(C)]
struct ArrayStorage<T, const N: usize> {
    entries: [MaybeUninit<T>; N],
    len: usize,
}

/// A consuming iterator over an [`ArrayVec`].
#[repr(C)]
pub struct ArrayVecIntoIter<T, const N: usize> {
    entries: [MaybeUninit<T>; N],
    index: usize,
    len: usize,
}

impl<T, const N: usize> ArrayVec<T, N> {
    pub fn new() -> Self {
        Self {
            storage: ArrayStorage::new(),
        }
    }

    pub fn from_fn(len: usize, mut f: impl FnMut(usize) -> T) -> Self {
        let mut values = Self::new();
        values.storage.fill_with(len, &mut f);
        values
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        self.storage.push(value)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.storage.pop()
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub const fn capacity(&self) -> usize {
        N
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

impl<T: Copy, const N: usize> CopyArrayVec<T, N> {
    pub fn new() -> Self {
        Self {
            storage: ArrayStorage::new(),
        }
    }

    pub fn from_fn(len: usize, mut f: impl FnMut(usize) -> T) -> Self {
        let mut values = Self::new();
        values.storage.fill_with(len, &mut f);
        values
    }

    pub fn push(&mut self, value: T) -> Result<(), T> {
        self.storage.push(value)
    }

    pub fn pop(&mut self) -> Option<T> {
        self.storage.pop()
    }

    pub fn len(&self) -> usize {
        self.storage.len()
    }

    pub const fn capacity(&self) -> usize {
        N
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

impl<T, const N: usize> ArrayStorage<T, N> {
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

    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        // SAFETY: the initialized prefix contained this element before `len`
        // was decremented; moving it transfers its drop obligation to caller.
        Some(unsafe { self.entries[self.len].assume_init_read() })
    }

    fn len(&self) -> usize {
        self.len
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }

    fn is_full(&self) -> bool {
        self.len == N
    }

    fn as_slice(&self) -> &[T] {
        // SAFETY: `0..len` is the initialized prefix and `MaybeUninit<T>` has
        // the same layout and alignment as `T`.
        unsafe { std::slice::from_raw_parts(self.entries.as_ptr().cast(), self.len) }
    }

    fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: `0..len` is the initialized prefix, this borrow is unique,
        // and `MaybeUninit<T>` has the same layout and alignment as `T`.
        unsafe { std::slice::from_raw_parts_mut(self.entries.as_mut_ptr().cast(), self.len) }
    }
}

impl<T, const N: usize> Default for ArrayVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy, const N: usize> Default for CopyArrayVec<T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, const N: usize> IntoIterator for ArrayVec<T, N> {
    type IntoIter = ArrayVecIntoIter<T, N>;
    type Item = T;

    fn into_iter(self) -> Self::IntoIter {
        let value = ManuallyDrop::new(self);
        let source = &value as *const ManuallyDrop<Self> as *const Self;
        // SAFETY: `value` suppresses ArrayVec's Drop. Moving the backing array
        // transfers its initialized prefix to the returned iterator exactly once.
        let entries = unsafe { core::ptr::read(core::ptr::addr_of!((*source).storage.entries)) };
        let len = unsafe { (*source).storage.len };
        ArrayVecIntoIter {
            entries,
            index: 0,
            len,
        }
    }
}

impl<T, const N: usize> Iterator for ArrayVecIntoIter<T, N> {
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

impl<T, const N: usize> DoubleEndedIterator for ArrayVecIntoIter<T, N> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.index == self.len {
            return None;
        }
        self.len -= 1;
        // SAFETY: `len` now names the last initialized, unread entry.
        Some(unsafe { self.entries[self.len].assume_init_read() })
    }
}

impl<T, const N: usize> ExactSizeIterator for ArrayVecIntoIter<T, N> {}
impl<T, const N: usize> FusedIterator for ArrayVecIntoIter<T, N> {}

impl<T, const N: usize> Drop for ArrayVec<T, N> {
    fn drop(&mut self) {
        let len = take(&mut self.storage.len);
        drop(DropEntries {
            entries: &mut self.storage.entries[..len],
        });
    }
}

impl<T, const N: usize> Drop for ArrayVecIntoIter<T, N> {
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
        while !self.entries.is_empty() {
            let entries = take(&mut self.entries);
            let (head, tail) = entries.split_first_mut().expect("non-empty entries");
            let mut remaining = Self { entries: tail };
            // SAFETY: this guard owns exactly the initialized suffix. On an
            // unwind, `remaining` drops the tail before propagating it.
            unsafe { head.assume_init_drop() };
            self.entries = take(&mut remaining.entries);
            forget(remaining);
        }
    }
}

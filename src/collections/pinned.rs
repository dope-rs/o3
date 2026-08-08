use std::pin::Pin;

/// A fixed, dense allocation whose elements remain pinned until drop.
#[repr(transparent)]
pub struct Slice<T> {
    entries: Pin<Box<[T]>>,
}

impl<T> Slice<T> {
    pub fn get(&self, index: usize) -> Option<Pin<&T>> {
        let entry = self.entries.as_ref().get_ref().get(index)?;
        // SAFETY: Slice never exposes ownership of or mutable unpinned access
        // to its boxed storage. The allocation remains pinned through the
        // returned shared borrow, so this element cannot move or be replaced.
        Some(unsafe { Pin::new_unchecked(entry) })
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<T> From<Box<[T]>> for Slice<T> {
    fn from(entries: Box<[T]>) -> Self {
        Self {
            entries: Box::into_pin(entries),
        }
    }
}

impl<T> FromIterator<T> for Slice<T> {
    fn from_iter<I: IntoIterator<Item = T>>(entries: I) -> Self {
        entries.into_iter().collect::<Box<[_]>>().into()
    }
}

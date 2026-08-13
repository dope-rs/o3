use std::pin;

/// A fixed, dense allocation whose elements remain pinned until drop.
#[repr(transparent)]
pub struct Slice<T> {
    entries: pin::Pin<Box<[T]>>,
}

impl<T> Slice<T> {
    #[inline]
    pub fn get(&self, index: usize) -> Option<pin::Pin<&T>> {
        let entry = self.entries.as_ref().get_ref().get(index)?;
        // SAFETY: Slice never exposes ownership of or mutable unpinned access
        // to its boxed storage. The allocation remains pinned through the
        // returned shared borrow, so this element cannot move or be replaced.
        Some(unsafe { pin::Pin::new_unchecked(entry) })
    }

    #[inline]
    pub fn get_mut(&mut self, index: usize) -> Option<pin::Pin<&mut T>> {
        if index >= self.entries.len() {
            return None;
        }
        // SAFETY: the projection stays within the pinned slice allocation and
        // the returned borrow prevents the element from being replaced or the
        // allocation from being dropped for its duration.
        Some(unsafe {
            self.entries
                .as_mut()
                .map_unchecked_mut(|entries| &mut entries[index])
        })
    }

    pub fn iter<'a>(
        &'a self,
    ) -> impl DoubleEndedIterator<Item = pin::Pin<&'a T>> + ExactSizeIterator + 'a
    where
        T: 'a,
    {
        self.entries.as_ref().get_ref().iter().map(|entry| {
            // SAFETY: Slice never exposes ownership of or mutable unpinned
            // access to its boxed storage. The allocation remains pinned
            // through the returned shared borrow, so this element cannot move
            // or be replaced.
            unsafe { pin::Pin::new_unchecked(entry) }
        })
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[inline]
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

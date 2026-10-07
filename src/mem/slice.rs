use std::ptr;

/// Slice operations that retain the boundary discovered in their original owner.
pub trait Ext<T> {
    fn split_while(&self, test: impl FnMut(&T) -> bool) -> (&[T], &[T]);

    /// Copies one admitted suffix to the start and returns that exact prefix.
    fn copy_tail(&mut self, start: usize) -> Option<&mut [T]>
    where
        T: Copy;
}

impl<T> Ext<T> for [T] {
    fn split_while(&self, mut test: impl FnMut(&T) -> bool) -> (&[T], &[T]) {
        let end = self.iter().take_while(|value| test(value)).count();
        // SAFETY: this traversal counts a prefix of this unchanged borrowed slice.
        unsafe { self.split_at_unchecked(end) }
    }

    fn copy_tail(&mut self, start: usize) -> Option<&mut [T]>
    where
        T: Copy,
    {
        let len = self.len().checked_sub(start)?;
        // SAFETY: admission bounds the suffix by this exclusive slice. Its
        // length also fits the destination prefix; ptr::copy permits overlap.
        unsafe {
            let target = self.as_mut_ptr();
            ptr::copy(target.add(start), target, len);
            Some(self.get_unchecked_mut(..len))
        }
    }
}

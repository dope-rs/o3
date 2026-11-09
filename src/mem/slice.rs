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
        self.copy_within(start.., 0);
        Some(&mut self[..len])
    }
}

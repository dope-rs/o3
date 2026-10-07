use crate::buffer::pool::owners;

/// Statically selected terminal policy for one exact pool backing kind.
/// # Safety
/// Release must destroy the allocation represented by this backing exactly once, after its final owner and every borrowed view have ended.
pub unsafe trait Backing {
    /// # Safety
    /// `core` must be the terminal allocation edge for this backing kind.
    unsafe fn release<S>(core: owners::Ptr<Self, S>)
    where
        Self: Sized;
}

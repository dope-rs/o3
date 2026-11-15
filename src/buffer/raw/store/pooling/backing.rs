use std::num;

use crate::buffer::pool::owners;

/// Statically selected terminal policy for one exact pool backing kind.
/// # Safety
/// Release must destroy the allocation represented by this backing exactly once, after its final owner and every borrowed view have ended.
/// Static dimensions match every allocation; a static slot count selects plain Header/Slot geometry.
/// COUNTED is true whenever a live Span can share its slot before consuming publication.
pub unsafe trait Backing {
    type Link: Clone;

    const COUNTED: bool = false;
    const SLOTS: Option<u32> = None;
    const CAPACITY: Option<num::NonZero<u32>> = None;

    /// # Safety
    /// `core` must be the terminal allocation edge for this backing kind.
    unsafe fn release<S>(core: owners::Ptr<Self, S>)
    where
        Self: Sized,
    {
        // SAFETY: the caller transfers this backing's terminal allocation edge.
        unsafe { core.deallocate() };
    }
}

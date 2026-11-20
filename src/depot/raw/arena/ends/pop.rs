use crate::depot;

/// # Safety
/// The implementation updates only its original native endpoint owner.
pub unsafe trait EndsPop<L: depot::raw::Link> {
    /// # Safety
    /// `next` is the stored successor of the just-detached current front.
    unsafe fn advance(self, next: L);
}

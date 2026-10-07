/// A detached node whose payload has not been initialized.
/// # Safety
/// Ready retains this same store, node, destination, and sole payload authority.
pub unsafe trait LaneInit: crate::depot::raw::TagEntry {
    type Ready: crate::depot::raw::TagTarget<Store = Self::Store>;
    /// # Safety
    /// The original slot's payload was written exactly once immediately before this transition.
    unsafe fn ready(self) -> Self::Ready;
}

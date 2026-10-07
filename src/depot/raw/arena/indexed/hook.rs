use crate::depot::queue::links;

/// Slot metadata policy retaining initialized membership and exact chain/head layout.
/// # Safety
/// The linked predicate precisely identifies initialized payloads; free successors preserve the admitted index domain.
pub unsafe trait IndexedHook {
    type Head: Default;
    type Chain: Default;
    fn new(index: links::Index, next: Option<links::Index>) -> Self;
    fn is_linked(&self, index: links::Index) -> bool;
    /// # Safety
    /// This hook is detached and belongs to the storage owner's free chain.
    unsafe fn next(&self) -> u32;
}

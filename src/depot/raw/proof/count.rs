use std::cell::Cell;

/// Optional cardinality storage for an indexed chain.
/// # Safety
/// EMPTY is zero and storage exposes this exact policy's counter, when counted.
pub unsafe trait ChainCount: Copy {
    const EMPTY: Self;
    fn storage(count: &Cell<Self>) -> Option<&Cell<u32>>;
}

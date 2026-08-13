use std::ptr;

use crate::collections::completion::narrow;

/// # Safety
/// The arena must stay live and exclusive until every issued echo completes.
pub unsafe trait ArenaOwner<
    'owner,
    T: Copy,
    const INDEX_BITS: u32 = 32,
    const GENERATION_BITS: u32 = 32,
>
{
    fn arena(self) -> ptr::NonNull<narrow::Arena<T, INDEX_BITS, GENERATION_BITS>>;
}

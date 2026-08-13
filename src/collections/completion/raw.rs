use std::ptr;

use crate::collections::completion;

/// # Safety
/// The arena must stay live and exclusive until every issued token completes.
pub unsafe trait ArenaOwner<'owner, T: Copy> {
    fn arena(self) -> ptr::NonNull<completion::Arena<T>>;
}

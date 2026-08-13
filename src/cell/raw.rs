use std::ptr;

/// # Safety
/// The pointer must remain valid and pinned while any derived link is usable.
pub unsafe trait StableLinkSource<T> {
    fn pointer(self) -> ptr::NonNull<T>;
}

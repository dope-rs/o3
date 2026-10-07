use std::mem;

/// Destruction selected by metadata co-owned with one initialized payload.
/// # Safety
/// Implementations destroy exactly the payload identified by their retained state.
pub unsafe trait State<V>: Copy {
    /// # Safety
    /// The payload must match this state and be initialized and destroyed exactly once.
    unsafe fn clear(self, value: &mut mem::ManuallyDrop<V>);
}

// SAFETY: ordinary values have one initialized payload and no additional state.
unsafe impl<V> State<V> for () {
    unsafe fn clear(self, value: &mut mem::ManuallyDrop<V>) {
        // SAFETY: the owner consumes its initialized payload exactly once.
        unsafe { mem::ManuallyDrop::drop(value) };
    }
}

use std::{mem, num::NonZero};

#[repr(C)]
pub struct StagedValue<Key, Queued, Parked> {
    pub(in crate::depot) key: mem::ManuallyDrop<Key>,
    value: Value<Queued, Parked>,
}

#[repr(C)]
union Value<Queued, Parked> {
    queued: mem::ManuallyDrop<Queued>,
    parked: mem::ManuallyDrop<Parked>,
}

impl<Key, Queued, Parked> StagedValue<Key, Queued, Parked> {
    pub fn queued(key: Key, value: Queued) -> Self {
        Self {
            key: mem::ManuallyDrop::new(key),
            value: Value {
                queued: mem::ManuallyDrop::new(value),
            },
        }
    }

    pub fn parked(key: Key, value: Parked) -> Self {
        Self {
            key: mem::ManuallyDrop::new(key),
            value: Value {
                parked: mem::ManuallyDrop::new(value),
            },
        }
    }

    pub fn key(&self) -> &Key {
        &self.key
    }

    /// # Safety
    /// The union must retain its initialized `Queued` field for the returned borrow.
    pub(in crate::depot) unsafe fn queued_ref(&self) -> &Queued {
        // SAFETY: the caller proves the union's active member is `queued`.
        unsafe { &self.value.queued }
    }

    /// # Safety
    /// The union must currently hold its `Queued` variant and remain borrowed exclusively.
    pub(in crate::depot) unsafe fn queued_mut(&mut self) -> &mut Queued {
        // SAFETY: the caller proves the union's active member is `queued`.
        unsafe { &mut self.value.queued }
    }

    /// # Safety
    /// The union must currently hold its `Parked` variant and remain borrowed exclusively.
    pub(in crate::depot) unsafe fn parked_mut(&mut self) -> &mut Parked {
        // SAFETY: the caller proves the union's active member is `parked`.
        unsafe { &mut self.value.parked }
    }

    /// # Safety
    /// The union must retain its initialized `Parked` field for the returned borrow.
    pub unsafe fn parked_ref(&self) -> &Parked {
        // SAFETY: the caller proves the union's active member is `parked`.
        unsafe { &self.value.parked }
    }

    /// # Safety
    /// The retained key must be initialized and released exactly once through this call.
    pub(in crate::depot) unsafe fn take_key(&mut self) -> Key {
        // SAFETY: the caller takes the key once, consuming its initialized `ManuallyDrop`.
        unsafe { mem::ManuallyDrop::take(&mut self.key) }
    }

    /// # Safety
    /// The union must hold its `Queued` variant, taken exactly once through this call.
    pub(in crate::depot) unsafe fn take_queued(&mut self) -> Queued {
        // SAFETY: the caller proves and consumes the active `queued` member once.
        unsafe { mem::ManuallyDrop::take(&mut self.value.queued) }
    }

    /// # Safety
    /// The union must hold its `Parked` variant, taken exactly once through this call.
    pub(in crate::depot) unsafe fn take_parked(&mut self) -> Parked {
        // SAFETY: the caller proves and consumes the active `parked` member once.
        unsafe { mem::ManuallyDrop::take(&mut self.value.parked) }
    }

    /// # Safety
    /// The key and the union's `Queued` variant must each be initialized and dropped exactly once.
    pub(in crate::depot) unsafe fn clear_queued(&mut self) {
        // SAFETY: the caller drops the initialized key and active `queued` member once.
        unsafe {
            mem::ManuallyDrop::drop(&mut self.key);
            mem::ManuallyDrop::drop(&mut self.value.queued);
        }
    }

    /// # Safety
    /// The key and the union's `Parked` variant must each be initialized and dropped exactly once.
    pub(in crate::depot) unsafe fn clear_parked(&mut self) {
        // SAFETY: the caller drops the initialized key and active `parked` member once.
        unsafe {
            mem::ManuallyDrop::drop(&mut self.key);
            mem::ManuallyDrop::drop(&mut self.value.parked);
        }
    }

    pub(in crate::depot) fn write_parked(&mut self, value: Parked) {
        self.value.parked = mem::ManuallyDrop::new(value);
    }

    pub(in crate::depot) fn write_queued(&mut self, value: Queued) {
        self.value.queued = mem::ManuallyDrop::new(value);
    }
}

/// # Safety
/// This dropless node has a private one-based free link: new initializes all fields with next() == None, and reset preserves its link. Every safely constructible external node has no link; safe replacement, cloning and payload operations preserve an exposed live node's absent link, and shared references cannot change links.
/// set_next stores the exact supplied link. Hooks return normally without callbacks or storage reentry; only the storage owner links free nodes, live nodes have no link, and each free chain ends with a link to its own slot.
pub unsafe trait LinkedNode: Sized {
    type Input: Copy;

    fn new(input: Self::Input) -> Self;
    fn reset(&mut self, input: Self::Input);
    fn next(&self) -> Option<NonZero<u32>>;

    /// # Safety
    /// The exclusive storage owner retains this initialized node and preserves
    /// its exact free-chain membership, live state, and sole return rights.
    unsafe fn set_next(&mut self, next: Option<NonZero<u32>>);
}

use std::{mem, num::NonZero};

#[repr(C)]
pub struct StagedValue<Key, Queued, Parked> {
    pub(in crate::depot) key: mem::ManuallyDrop<Key>,
    pub(in crate::depot) value: Value<Queued, Parked>,
}

#[repr(C)]
pub(in crate::depot) union Value<Queued, Parked> {
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
    /// The union must retain its initialized `Parked` field for the returned borrow.
    pub unsafe fn parked_ref(&self) -> &Parked {
        // SAFETY: the caller proves the union's active member is `parked`.
        unsafe { &self.value.parked }
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

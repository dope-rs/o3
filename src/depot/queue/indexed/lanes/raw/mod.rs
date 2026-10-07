use std::mem;

pub(in crate::depot) use state::State;

mod state;

/// A payload and its destruction state transferred together between exact owners.
pub(in crate::depot) struct Owned<V, S: State<V> = ()> {
    value: mem::ManuallyDrop<V>,
    state: S,
}

impl<V> Owned<V> {
    pub(in crate::depot) fn new(value: V) -> Self {
        Self {
            value: mem::ManuallyDrop::new(value),
            state: (),
        }
    }

    pub(in crate::depot) fn into_value(self) -> V {
        self.into_parts().0
    }
}

impl<V, S: State<V>> Owned<V, S> {
    /// # Safety
    /// The initialized payload must match its destruction state.
    pub(in crate::depot) unsafe fn from_parts(value: V, state: S) -> Self {
        Self {
            value: mem::ManuallyDrop::new(value),
            state,
        }
    }

    pub(in crate::depot) fn into_parts(self) -> (V, S) {
        let mut owner = mem::ManuallyDrop::new(self);
        // SAFETY: this consumed owner transfers its sole initialized payload and
        // the metadata that identifies it, suppressing its former destructor.
        (
            unsafe { mem::ManuallyDrop::take(&mut owner.value) },
            owner.state,
        )
    }
}

impl<V, S: State<V>> Drop for Owned<V, S> {
    fn drop(&mut self) {
        // SAFETY: construction retained the exact initialized state-payload pair.
        unsafe { self.state.clear(&mut self.value) };
    }
}

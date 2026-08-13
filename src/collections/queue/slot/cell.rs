use crate::collections::{self, queue::slot};

/// A fixed-capacity indexed queue with shared mutation and no borrowed values.
pub struct Cell<T: Copy = ()> {
    core: slot::Core<T, slot::SharedMode>,
}

impl<T: Copy> Cell<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        match Self::try_with_capacity(capacity) {
            Ok(queue) => queue,
            Err(error) => error.abort(),
        }
    }

    pub fn try_with_capacity(capacity: usize) -> Result<Self, collections::AllocationError> {
        Ok(Self {
            core: slot::Core::try_with_capacity(capacity)?,
        })
    }

    pub fn push_back(&self, index: usize, value: T) -> Result<(), T> {
        let queue: slot::Shared<'_, T> = self.core.shared();
        queue.push_back(index, value)
    }

    pub fn pop_front(&self) -> Option<T> {
        let queue: slot::Shared<'_, T> = self.core.shared();
        queue.pop_front()
    }

    pub fn remove(&self, index: usize) -> Option<T> {
        let queue: slot::Shared<'_, T> = self.core.shared();
        queue.remove(index)
    }

    pub fn remove_if(&self, index: usize, predicate: impl FnOnce(&T) -> bool) -> Option<T> {
        let queue: slot::Shared<'_, T> = self.core.shared();
        queue.remove_if(index, predicate)
    }

    pub fn is_empty(&self) -> bool {
        self.core.is_empty()
    }
}

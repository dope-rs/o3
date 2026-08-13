use crate::collections::{self, queue::slot};

pub struct Fifo<T = ()> {
    core: slot::Core<T, slot::ExclusiveMode>,
}

impl<T> Fifo<T> {
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

    pub fn vacant_entry(&mut self, index: usize) -> Option<slot::Vacant<'_, T>> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.vacant(index).map(slot::Vacant::new)
    }

    pub fn push_back(&mut self, index: usize, value: T) -> Result<(), T> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.push_back(index, value)
    }

    pub fn push_front(&mut self, index: usize, value: T) -> Result<(), T> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.push_front(index, value)
    }

    pub fn refresh_back(&mut self, index: usize, value: T) -> Result<Option<T>, T> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.refresh_back(index, value)
    }

    pub fn pop_front(&mut self) -> Option<T> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.pop_front()
    }

    pub fn front_key_value(&self) -> Option<(usize, &T)> {
        self.core.front_key_value()
    }

    pub fn front_entry(&mut self) -> Option<slot::Front<'_, T>> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.front_entry().map(slot::Front::new)
    }

    pub fn pop_front_key_value(&mut self) -> Option<(usize, T)> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.pop_front_key_value()
    }

    pub fn remove(&mut self, index: usize) -> Option<T> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.remove(index)
    }

    pub fn remove_if(&mut self, index: usize, predicate: impl FnOnce(&T) -> bool) -> Option<T> {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.remove_if(index, predicate)
    }

    pub fn clear(&mut self) {
        let queue: slot::Write<'_, T> = self.core.write();
        queue.clear();
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    pub fn len(&self) -> usize {
        self.core.len()
    }

    pub fn is_empty(&self) -> bool {
        self.core.is_empty()
    }
}

impl<T> Drop for Fifo<T> {
    fn drop(&mut self) {
        collections::ClearGuard::run(self, Self::clear);
    }
}

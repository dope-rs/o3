use std::{cell, marker, rc};

use crate::{
    buffer::{self, storage, view},
    cell::region,
};

struct Core {
    capacity: usize,
    resident: cell::Cell<usize>,
}

pub struct Budget<'d> {
    core: rc::Rc<Core>,
    brand: marker::PhantomData<*mut &'d ()>,
}

pub struct Charge {
    core: rc::Rc<Core>,
    capacity: usize,
}

pub(in crate::buffer) struct Lease {
    core: rc::Rc<Core>,
    capacity: u32,
}

impl<'d> Budget<'d> {
    pub fn new(capacity: usize, token: &region::Token<'d>) -> Self {
        let _ = token;
        Self {
            core: rc::Rc::new(Core {
                capacity,
                resident: cell::Cell::new(0),
            }),
            brand: marker::PhantomData,
        }
    }

    pub fn capacity(&self) -> usize {
        self.core.capacity
    }

    pub fn resident(&self) -> usize {
        self.core.resident.get()
    }

    pub fn available(&self) -> usize {
        self.core.capacity - self.core.resident.get()
    }

    pub fn try_charge(&self, capacity: usize) -> Result<Charge, buffer::CapacityError> {
        let resident = self.core.resident.get();
        let attempted = resident
            .checked_add(capacity)
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, self.core.capacity))?;
        if attempted > self.core.capacity {
            return Err(buffer::CapacityError::new(attempted, self.core.capacity));
        }
        self.core.resident.set(attempted);
        Ok(Charge {
            core: rc::Rc::clone(&self.core),
            capacity,
        })
    }

    pub(in crate::buffer) fn acquire(&self, capacity: u32) -> Result<Lease, buffer::CapacityError> {
        Lease::acquire(rc::Rc::clone(&self.core), capacity)
    }

    pub(in crate::buffer) fn acquire_zero(&self) -> Lease {
        Lease {
            core: rc::Rc::clone(&self.core),
            capacity: 0,
        }
    }
}

impl Drop for Charge {
    fn drop(&mut self) {
        let resident = self.core.resident.get();
        assert!(resident >= self.capacity, "resident charge underflow");
        self.core.resident.set(resident - self.capacity);
    }
}

impl Lease {
    fn acquire(core: rc::Rc<Core>, capacity: u32) -> Result<Self, buffer::CapacityError> {
        let resident = core.resident.get();
        let attempted = resident
            .checked_add(capacity as usize)
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, core.capacity))?;
        if attempted > core.capacity {
            return Err(buffer::CapacityError::new(attempted, core.capacity));
        }
        core.resident.set(attempted);
        Ok(Self { core, capacity })
    }

    pub(in crate::buffer) fn sibling(&self, capacity: u32) -> Result<Self, buffer::CapacityError> {
        Self::acquire(rc::Rc::clone(&self.core), capacity)
    }

    pub(in crate::buffer) fn grow(&mut self, capacity: u32) -> Result<(), buffer::CapacityError> {
        if capacity <= self.capacity {
            return Ok(());
        }
        let additional = (capacity - self.capacity) as usize;
        let resident = self.core.resident.get();
        let attempted = resident
            .checked_add(additional)
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, self.core.capacity))?;
        if attempted > self.core.capacity {
            return Err(buffer::CapacityError::new(attempted, self.core.capacity));
        }
        self.core.resident.set(attempted);
        self.capacity = capacity;
        Ok(())
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let resident = self.core.resident.get();
        debug_assert!(resident >= self.capacity as usize);
        self.core.resident.set(resident - self.capacity as usize);
    }
}

pub struct Snapshot<'d, const MAX_CAPACITY: usize> {
    raw: view::Raw<Lease, MAX_CAPACITY>,
    brand: marker::PhantomData<*mut &'d ()>,
}

impl<'d, const MAX_CAPACITY: usize> Snapshot<'d, MAX_CAPACITY> {
    pub fn new(budget: &Budget<'d>) -> Self {
        Self {
            raw: view::Raw::new_accounted(budget),
            brand: marker::PhantomData,
        }
    }

    pub fn with_capacity_up_to(
        budget: &Budget<'d>,
        requested: usize,
    ) -> Result<Self, buffer::CapacityError> {
        view::Raw::<Lease, MAX_CAPACITY>::validate();
        let capacity = requested.min(MAX_CAPACITY);
        Ok(Self {
            raw: view::Raw::with_accounted_capacity(budget, capacity)?,
            brand: marker::PhantomData,
        })
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        self.raw.try_extend(src)
    }

    pub fn try_reserve_to(&mut self, target: usize) -> Result<(), buffer::CapacityError> {
        self.raw.try_reserve_to(target)
    }

    pub fn snapshot(&self) -> Option<storage::Shared> {
        self.raw.span().map(storage::Shared::from_resident_span)
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn compact(&mut self) -> Result<(), buffer::CapacityError> {
        self.raw.compact()
    }

    pub fn release_empty(&mut self) {
        self.raw.release_empty();
    }
}

impl<const MAX_CAPACITY: usize> buffer::PrefixLength for Snapshot<'_, MAX_CAPACITY> {
    fn prefix_len(&self) -> usize {
        self.raw.len()
    }
}

impl<const MAX_CAPACITY: usize> buffer::PrefixConsumer for Snapshot<'_, MAX_CAPACITY> {
    fn consume_validated_prefix(&mut self, proof: buffer::PrefixProof) {
        self.raw.consume_valid(proof.amount());
    }
}

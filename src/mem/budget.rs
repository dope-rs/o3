use std::{
    cell::Cell,
    marker::{PhantomData, PhantomPinned},
    pin::Pin,
    ptr::NonNull,
};

use crate::ThreadBound;

pub struct Bytes {
    limit: usize,
    used: Cell<usize>,
    _pin: PhantomPinned,
    _thread: ThreadBound,
}

impl Bytes {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            used: Cell::new(0),
            _pin: PhantomPinned,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn handle<'d>(self: Pin<&'d Self>) -> Handle<'d> {
        Handle(NonNull::from(self.get_ref()), PhantomData)
    }
}

#[derive(Clone, Copy)]
pub struct Handle<'d>(NonNull<Bytes>, PhantomData<&'d Bytes>);

impl<'d> Handle<'d> {
    fn budget(self) -> &'d Bytes {
        unsafe { self.0.as_ref() }
    }

    pub fn try_acquire(self, amount: usize) -> Option<Lease<'d>> {
        let budget = self.budget();
        let used = budget.used.get().checked_add(amount)?;
        if used > budget.limit {
            return None;
        }
        budget.used.set(used);
        Some(Lease {
            budget: self,
            amount,
        })
    }

    fn release(self, amount: usize) {
        let budget = self.budget();
        assert!(budget.used.get() >= amount, "byte budget underflow");
        budget.used.set(budget.used.get() - amount);
    }
}

pub struct Lease<'d> {
    budget: Handle<'d>,
    amount: usize,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.budget.release(self.amount);
    }
}

use std::cell::Cell;
use std::marker::{PhantomData, PhantomPinned};
use std::pin::Pin;
use std::ptr::NonNull;

use crate::marker::ThreadBound;

pub struct ByteBudget {
    limit: usize,
    used: Cell<usize>,
    _pin: PhantomPinned,
    _thread: ThreadBound,
}

impl ByteBudget {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            used: Cell::new(0),
            _pin: PhantomPinned,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn used(&self) -> usize {
        self.used.get()
    }

    pub fn handle<'d>(self: Pin<&'d Self>) -> ByteBudgetHandle<'d> {
        ByteBudgetHandle(NonNull::from(self.get_ref()), PhantomData)
    }

    pub fn try_acquire(self: Pin<&Self>, amount: usize) -> Option<ByteLease<'_>> {
        self.handle().try_acquire(amount)
    }
}

#[derive(Clone, Copy)]
pub struct ByteBudgetHandle<'d>(NonNull<ByteBudget>, PhantomData<&'d ByteBudget>);

impl<'d> ByteBudgetHandle<'d> {
    fn budget(self) -> &'d ByteBudget {
        unsafe { self.0.as_ref() }
    }

    pub fn limit(self) -> usize {
        self.budget().limit
    }

    pub fn used(self) -> usize {
        self.budget().used.get()
    }

    pub fn try_acquire(self, amount: usize) -> Option<ByteLease<'d>> {
        let budget = self.budget();
        let used = budget.used.get().checked_add(amount)?;
        if used > budget.limit {
            return None;
        }
        budget.used.set(used);
        Some(ByteLease {
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

pub struct ByteLease<'d> {
    budget: ByteBudgetHandle<'d>,
    amount: usize,
}

impl ByteLease<'_> {
    pub fn amount(&self) -> usize {
        self.amount
    }

    pub fn shrink(&mut self, amount: usize) {
        assert!(amount <= self.amount, "byte lease underflow");
        self.amount -= amount;
        self.budget.release(amount);
    }
}

impl Drop for ByteLease<'_> {
    fn drop(&mut self) {
        self.budget.release(self.amount);
    }
}

use std::{cell, num};

use crate::{
    base::marker::ThreadBound,
    mem::{credit, quota::Take},
};

/// Internal scalar account. `HELD` retains usage instead of available credit.
/// Public owners retain every return authority.
#[derive(Debug)]
#[repr(transparent)]
pub(crate) struct Account<const HELD: bool = false> {
    value: cell::Cell<usize>,
    thread: ThreadBound,
}

impl<const HELD: bool> Account<HELD> {
    pub(crate) const fn new(value: usize) -> Self {
        Self {
            value: cell::Cell::new(value),
            thread: ThreadBound::NEW,
        }
    }

    pub(crate) fn get(&self) -> usize {
        self.value.get()
    }

    /// Commits admission before the owner can invoke reentrant work.
    pub(crate) fn try_acquire(
        &self,
        amount: num::NonZero<usize>,
        limit: usize,
    ) -> Result<(), credit::Error> {
        let current = self.value.get();
        let spare = if HELD { limit - current } else { current };
        if amount.get() > spare {
            return Err(credit::Error::new(amount, spare));
        }
        self.value.set(if HELD {
            current + amount.get()
        } else {
            current - amount.get()
        });
        Ok(())
    }

    /// The private owner consumes this exact account's outstanding debit.
    pub(crate) fn refund(&self, amount: usize) {
        let current = self.value.get();
        self.value.set(if HELD {
            current.wrapping_sub(amount)
        } else {
            current.wrapping_add(amount)
        });
    }
}

impl Account {
    pub(crate) fn reserve(&self, amount: usize) -> Take {
        let Some(left) = self.value.get().checked_sub(amount) else {
            return Take::Exhausted;
        };
        self.value.set(left);
        Take::Taken
    }

    pub(crate) fn reserve_up(&self, limit: usize) -> usize {
        let spare = self.value.get();
        let amount = spare.min(limit);
        if amount != 0 {
            self.value.set(spare - amount);
        }
        amount
    }

    pub(in crate::mem) fn refund_spare(&self, amount: usize) {
        if amount != 0 {
            self.refund(amount);
        }
    }
}

const _: () = assert!(size_of::<Account>() == size_of::<usize>());
const _: () = assert!(size_of::<Account<true>>() == size_of::<usize>());

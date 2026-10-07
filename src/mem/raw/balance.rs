use std::{array, hint};

use crate::mem::{self, credit::fair};

/// Pure accounting values; source ownership and commit authority stay with the caller.
#[derive(Clone, Copy, Debug)]
#[must_use]
pub struct Balance<const N: usize, W: mem::raw::Word> {
    held: [W; N],
    shared: [W; N],
}

impl<const N: usize, W: mem::raw::Word> Balance<N, W> {
    pub const fn new(held: [W; N], shared: [W; N]) -> Self {
        const { assert!(N != 0) };
        Self { held, shared }
    }

    pub fn into_parts(self) -> ([W; N], [W; N]) {
        (self.held, self.shared)
    }

    /// Admits the exact amount without changing source accounts.
    pub fn try_acquire(self, reserve: [W; N], amount: [W; N]) -> Result<Self, fair::Cause<W, N>> {
        let mut shared = self.shared;
        let mut next = self.held;
        for index in 0..N {
            let current = self.held[index];
            let request = amount[index];
            let Some(total) = current.checked_add(request) else {
                hint::cold_path();
                return Err(fair::Cause::Overflow {
                    axis: fair::Axis::<N>::ALL[index],
                    held: current,
                    requested: request,
                });
            };
            let required = Self::required(current, reserve[index], request);
            if required > shared[index] {
                return Err(fair::Cause::Exhausted {
                    axis: fair::Axis::<N>::ALL[index],
                    required,
                    available: shared[index],
                });
            }
            next[index] = total;
            shared[index] = shared[index] - required;
        }
        Ok(Self::new(next, shared))
    }

    /// # Safety
    /// Exact held-plus-amount sums fit `W`, and every shared demand is available.
    /// Sealed word operations cannot reenter between the admitted reads and commit.
    pub unsafe fn acquire_raw(self, reserve: [W; N], amount: [W; N]) -> Self {
        let mut next = self.held;
        let mut shared = self.shared;
        for axis in 0..N {
            let total = self.held[axis] + amount[axis];
            let required = Self::required(self.held[axis], reserve[axis], amount[axis]);
            next[axis] = total;
            shared[axis] = shared[axis] - required;
        }
        Self::new(next, shared)
    }

    /// Computes incremental shared demand without requiring the requested sum to fit.
    /// This does not admit the request or change either account.
    pub fn demand(self, reserve: [W; N], amount: [W; N]) -> [W; N] {
        array::from_fn(|axis| Self::required(self.held[axis], reserve[axis], amount[axis]))
    }

    /// # Safety
    /// This state holds the amount; returning its disjoint acquisition conserves
    /// the issuing shared capacity, so every refund fits `W` regardless of order.
    pub unsafe fn release_raw(self, reserve: [W; N], amount: [W; N]) -> Self {
        let mut next = self.held;
        let mut shared = self.shared;
        for axis in 0..N {
            next[axis] = self.held[axis] - amount[axis];
            let returned = self.held[axis].sub_or_zero(reserve[axis]).min(amount[axis]);
            shared[axis] = shared[axis] + returned;
        }
        Self::new(next, shared)
    }

    fn required(held: W, reserve: W, amount: W) -> W {
        amount.sub_or_zero(reserve.sub_or_zero(held))
    }
}

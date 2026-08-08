pub mod bytes;
pub mod pool;
pub mod queue;
mod seal;
pub mod storage;
pub mod view;
pub mod write;

use std::{error, fmt, mem, ops, ptr};

pub(crate) use seal::Seal;

/// Reports the logical byte prefix that an exclusive owner may consume.
pub trait PrefixLength {
    fn prefix_len(&self) -> usize;
}

/// Owns a logical byte prefix that can be consumed after it is proven to fit.
/// Only [`ValidatedPrefix`] can construct the proof passed to this method.
pub trait PrefixConsumer: PrefixLength {
    fn consume_validated_prefix(&mut self, proof: PrefixProof);

    fn try_consume_prefix(
        &mut self,
        amount: usize,
    ) -> Result<ValidatedPrefix<'_, Self>, CapacityError> {
        ValidatedPrefix::try_new(self, amount)
    }

    fn consume_prefix_up_to(&mut self, requested: usize) -> usize {
        let prefix = ValidatedPrefix::up_to(self, requested);
        let amount = prefix.len();
        prefix.commit();
        amount
    }
}

/// An unforgeable proof that one prefix fits its exclusively borrowed owner.
#[doc(hidden)]
pub struct PrefixProof {
    amount: usize,
}

impl PrefixProof {
    fn new(amount: usize) -> Self {
        Self { amount }
    }

    pub const fn amount(&self) -> usize {
        self.amount
    }
}

/// Proof that `amount` fits one exclusively borrowed target prefix.
#[must_use]
pub struct ValidatedPrefix<'a, T: PrefixConsumer + ?Sized> {
    target: &'a mut T,
    proof: PrefixProof,
}

impl<'a, T: PrefixConsumer + ?Sized> ValidatedPrefix<'a, T> {
    fn try_new(target: &'a mut T, amount: usize) -> Result<Self, CapacityError> {
        let available = target.prefix_len();
        if amount > available {
            return Err(CapacityError::new(amount, available));
        }
        Ok(Self {
            target,
            proof: PrefixProof::new(amount),
        })
    }

    /// Proves the largest prefix no longer than `requested`.
    fn up_to(target: &'a mut T, requested: usize) -> Self {
        let amount = requested.min(target.prefix_len());
        Self {
            target,
            proof: PrefixProof::new(amount),
        }
    }

    const fn len(&self) -> usize {
        self.proof.amount()
    }
}

impl<T: PrefixConsumer + ?Sized> ValidatedPrefix<'_, T> {
    /// Applies the validated mutation exactly once.
    pub fn commit(self) {
        self.target.consume_validated_prefix(self.proof);
    }
}

pub const BLOCK_CAPACITY: u32 = 64 * 1024;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CapacityError {
    attempted: usize,
    capacity: usize,
}

impl CapacityError {
    const fn new(attempted: usize, capacity: usize) -> Self {
        Self {
            attempted,
            capacity,
        }
    }
}

impl fmt::Debug for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CapacityError")
            .field("attempted", &self.attempted)
            .field("capacity", &self.capacity)
            .finish()
    }
}

impl fmt::Display for CapacityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "capacity exceeded: attempted {}, capacity {}",
            self.attempted, self.capacity
        )
    }
}

impl error::Error for CapacityError {}

fn checked_append_len<const N: usize>(
    start: usize,
    capacity: usize,
    slices: &[&[u8]; N],
) -> Result<usize, CapacityError> {
    let mut end = start;
    for slice in slices {
        end = end
            .checked_add(slice.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(CapacityError::new(end, capacity));
        }
    }
    Ok(end)
}

trait RangeExt {
    fn is_within(&self, len: usize) -> bool;
}

impl RangeExt for ops::Range<usize> {
    fn is_within(&self, len: usize) -> bool {
        self.start <= self.end && self.end <= len
    }
}

/// # Safety
/// `buf` is valid through `*tail`, and `*head <= *tail`.
unsafe fn compact(buf: *mut mem::MaybeUninit<u8>, head: &mut u32, tail: &mut u32) {
    if *head == 0 {
        return;
    }
    let len = (*tail - *head) as usize;
    if len != 0 {
        unsafe { ptr::copy(buf.add(*head as usize), buf, len) };
    }
    *head = 0;
    *tail = len as u32;
}

/// # Safety
/// `amount <= *tail - *head`.
unsafe fn consume(head: &mut u32, tail: &mut u32, amount: usize) {
    *head = head.wrapping_add(amount as u32);
    if *head == *tail {
        *head = 0;
        *tail = 0;
    }
}

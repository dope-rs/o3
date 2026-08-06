use std::{mem::MaybeUninit, num::NonZeroUsize, ptr::copy_nonoverlapping, slice::from_raw_parts};

use crate::{
    ThreadBound,
    buffer::{CapacityError, PrefixConsumer, PrefixLength, PrefixProof, checked_append_len},
};

const fn wrap(index: usize, capacity: usize) -> usize {
    if index >= capacity {
        index - capacity
    } else {
        index
    }
}

pub struct Ring {
    buf: Box<[MaybeUninit<u8>]>,
    head: usize,
    len: usize,
    _thread: ThreadBound,
}

impl Ring {
    #[must_use]
    pub fn with_capacity(capacity: NonZeroUsize) -> Self {
        Self {
            buf: Box::<[u8]>::new_uninit_slice(capacity.get()),
            head: 0,
            len: 0,
            _thread: ThreadBound::NEW,
        }
    }

    pub fn capacity(&self) -> usize {
        self.buf.len()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn remaining(&self) -> usize {
        self.capacity() - self.len
    }

    pub fn as_slices(&self) -> (&[u8], &[u8]) {
        let first_len = self.len.min(self.capacity() - self.head);
        let second_len = self.len - first_len;
        unsafe {
            (
                from_raw_parts(self.buf.as_ptr().add(self.head).cast(), first_len),
                from_raw_parts(self.buf.as_ptr().cast(), second_len),
            )
        }
    }

    /// Appends one contiguous slice after validating its complete length.
    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let capacity = self.capacity();
        let end = self
            .len
            .checked_add(src.len())
            .ok_or_else(|| CapacityError::new(usize::MAX, capacity))?;
        if end > capacity {
            return Err(CapacityError::new(end, capacity));
        }
        let tail = wrap(self.head + self.len, capacity);
        self.copy_at_tail(tail, src);
        self.len = end;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        let capacity = self.capacity();
        if self.len == capacity {
            return Err(CapacityError::new(self.len + 1, capacity));
        }
        let tail = wrap(self.head + self.len, capacity);
        unsafe {
            self.buf
                .as_mut_ptr()
                .add(tail)
                .write(MaybeUninit::new(byte))
        };
        self.len += 1;
        Ok(())
    }

    /// Appends every slice after validating their aggregate length.
    ///
    /// On failure, the ring and its logical length are unchanged.
    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), CapacityError> {
        let capacity = self.capacity();
        let end = checked_append_len(self.len, capacity, &slices)?;
        let mut tail = wrap(self.head + self.len, capacity);
        for src in slices {
            self.copy_at_tail(tail, src);
            tail = wrap(tail + src.len(), capacity);
        }
        self.len = end;
        Ok(())
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len);
        self.head = wrap(self.head + amount, self.capacity());
        self.len -= amount;
        if self.is_empty() {
            self.head = 0;
        }
    }

    fn copy_at_tail(&mut self, tail: usize, src: &[u8]) {
        let first_len = src.len().min(self.capacity() - tail);
        unsafe {
            copy_nonoverlapping(
                src.as_ptr(),
                self.buf.as_mut_ptr().add(tail).cast(),
                first_len,
            );
            let second_len = src.len() - first_len;
            if second_len != 0 {
                copy_nonoverlapping(
                    src.as_ptr().add(first_len),
                    self.buf.as_mut_ptr().cast(),
                    second_len,
                );
            }
        }
    }
}

impl PrefixLength for Ring {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl PrefixConsumer for Ring {
    fn consume_validated_prefix(&mut self, proof: PrefixProof) {
        self.consume_valid(proof.amount());
    }
}

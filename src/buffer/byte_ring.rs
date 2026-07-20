use std::mem::MaybeUninit;
use std::ptr::copy_nonoverlapping;

use crate::marker::ThreadBound;

use super::CapacityError;

macro_rules! wrap {
    ($index:expr, $capacity:expr) => {{
        let index = $index;
        let capacity = $capacity;
        if index >= capacity {
            index - capacity
        } else {
            index
        }
    }};
}

pub struct ByteRing {
    buf: Box<[MaybeUninit<u8>]>,
    head: usize,
    len: usize,
    _thread: ThreadBound,
}

impl ByteRing {
    pub fn with_capacity(capacity: usize) -> Self {
        assert!(capacity > 0, "byte ring capacity must be positive");
        Self {
            buf: Box::<[u8]>::new_uninit_slice(capacity),
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
                std::slice::from_raw_parts(self.buf.as_ptr().add(self.head).cast(), first_len),
                std::slice::from_raw_parts(self.buf.as_ptr().cast(), second_len),
            )
        }
    }

    pub fn range_slices(&self, offset: usize, len: usize) -> Option<(&[u8], &[u8])> {
        let end = offset.checked_add(len)?;
        if end > self.len {
            return None;
        }
        let start = wrap!(self.head + offset, self.capacity());
        let first_len = len.min(self.capacity() - start);
        let second_len = len - first_len;
        unsafe {
            Some((
                std::slice::from_raw_parts(self.buf.as_ptr().add(start).cast(), first_len),
                std::slice::from_raw_parts(self.buf.as_ptr().cast(), second_len),
            ))
        }
    }

    pub fn copy_range_into(&self, offset: usize, dst: &mut [u8]) -> bool {
        let Some((first, second)) = self.range_slices(offset, dst.len()) else {
            return false;
        };
        dst[..first.len()].copy_from_slice(first);
        dst[first.len()..].copy_from_slice(second);
        true
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        if src.len() > self.remaining() {
            return Err(CapacityError::new(
                self.len.saturating_add(src.len()),
                self.capacity(),
            ));
        }
        let tail = wrap!(self.head + self.len, self.capacity());
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
        self.len += src.len();
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        if self.len == self.capacity() {
            return Err(CapacityError::new(self.len + 1, self.capacity()));
        }
        let tail = wrap!(self.head + self.len, self.capacity());
        self.buf[tail].write(byte);
        self.len += 1;
        Ok(())
    }

    pub fn consume(&mut self, amount: usize) {
        assert!(amount <= self.len, "byte ring consume past end");
        self.head = wrap!(self.head + amount, self.capacity());
        self.len -= amount;
        if self.len == 0 {
            self.head = 0;
        }
    }
}

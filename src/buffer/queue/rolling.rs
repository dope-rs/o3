use crate::marker::ThreadBound;
use std::mem::MaybeUninit;
use std::ptr::{addr_of_mut, copy_nonoverlapping};

use super::super::{CapacityError, PrefixLength, SpareWriter, compact, consume};

pub struct RollingBuffer<const CAP: usize> {
    buf: [MaybeUninit<u8>; CAP],
    head: u32,
    tail: u32,
    _thread: ThreadBound,
}

impl<const CAP: usize> Default for RollingBuffer<CAP> {
    fn default() -> Self {
        let () = Self::VALID;
        Self {
            buf: [MaybeUninit::uninit(); CAP],
            head: 0,
            tail: 0,
            _thread: ThreadBound::NEW,
        }
    }
}

impl<const CAP: usize> RollingBuffer<CAP> {
    const VALID: () = assert!(
        CAP <= u32::MAX as usize,
        "buffer::RollingBuffer CAP must fit u32"
    );

    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn new_boxed() -> Box<Self> {
        let () = Self::VALID;
        let mut value = Box::<Self>::new_uninit();
        let ptr = value.as_mut_ptr();
        unsafe {
            addr_of_mut!((*ptr).head).write(0);
            addr_of_mut!((*ptr).tail).write(0);
            addr_of_mut!((*ptr)._thread).write(ThreadBound::NEW);
            value.assume_init()
        }
    }

    pub const fn capacity(&self) -> usize {
        CAP
    }

    pub fn len(&self) -> usize {
        (self.tail - self.head) as usize
    }

    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    pub fn spare_capacity(&self) -> usize {
        CAP - self.len()
    }

    pub fn as_slice(&self) -> &[u8] {
        let h = self.head as usize;
        let t = self.tail as usize;
        unsafe {
            use std::slice::from_raw_parts;
            from_raw_parts(self.buf.as_ptr().add(h).cast(), t - h)
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let h = self.head as usize;
        let t = self.tail as usize;
        unsafe {
            use std::slice::from_raw_parts_mut;
            from_raw_parts_mut(self.buf.as_mut_ptr().add(h).cast(), t - h)
        }
    }

    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        if (CAP - self.tail as usize) < self.spare_capacity() {
            self.compact();
        }
        let t = self.tail as usize;
        let ptr = unsafe { self.buf.as_mut_ptr().add(t) };
        unsafe { SpareWriter::new(ptr, CAP - t, &mut self.tail) }
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let need = src.len();
        if need == 0 {
            return Ok(());
        }
        let len = self.len();
        let attempted = len
            .checked_add(need)
            .ok_or_else(|| CapacityError::new(usize::MAX, CAP))?;
        if attempted > CAP {
            return Err(CapacityError::new(attempted, CAP));
        }
        let tail_room = CAP - self.tail as usize;
        if need > tail_room {
            self.compact();
        }
        let tail = self.tail as usize;
        unsafe {
            copy_nonoverlapping(src.as_ptr(), self.buf.as_mut_ptr().add(tail).cast(), need);
        }
        self.tail = (tail + need) as u32;
        Ok(())
    }

    pub fn try_consume(&mut self, n: usize) -> Result<(), CapacityError> {
        let len = self.len();
        if n > len {
            return Err(CapacityError::new(n, len));
        }
        self.consume_valid(n);
        Ok(())
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        unsafe { consume(&mut self.head, &mut self.tail, amount) };
    }

    super::super::prefix::consume_prefix_api!(Self::consume_valid);

    #[cold]
    fn compact(&mut self) {
        unsafe { compact(self.buf.as_mut_ptr(), &mut self.head, &mut self.tail) };
    }
}

impl<const CAP: usize> PrefixLength for RollingBuffer<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

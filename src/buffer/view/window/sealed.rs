use std::mem;

use crate::buffer;

pub struct Inline<const CAP: usize> {
    buf: [mem::MaybeUninit<u8>; CAP],
    head: u32,
    tail: u32,
    _thread: crate::ThreadBound,
}

impl<const CAP: usize> Default for Inline<CAP> {
    fn default() -> Self {
        let () = Self::VALID;
        Self {
            buf: [mem::MaybeUninit::uninit(); CAP],
            head: 0,
            tail: 0,
            _thread: Default::default(),
        }
    }
}

impl<const CAP: usize> Inline<CAP> {
    const VALID: () = assert!(CAP <= u32::MAX as usize, "buffer::Inline CAP must fit u32");

    #[must_use]
    pub fn new_boxed() -> Box<Self> {
        let () = Self::VALID;
        let mut value = Box::<Self>::new_uninit();
        let ptr = value.as_mut_ptr();
        unsafe {
            use std::ptr::addr_of_mut;
            addr_of_mut!((*ptr).head).write(0);
            addr_of_mut!((*ptr).tail).write(0);
            addr_of_mut!((*ptr)._thread).write(Default::default());
            value.assume_init()
        }
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

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        use crate::buffer::CapacityError;
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
            use std::ptr::copy_nonoverlapping;
            copy_nonoverlapping(src.as_ptr(), self.buf.as_mut_ptr().add(tail).cast(), need);
        }
        self.tail = (tail + need) as u32;
        Ok(())
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        self.head = self.head.wrapping_add(amount as u32);
        if self.head == self.tail {
            self.head = 0;
            self.tail = 0;
        }
    }

    #[cold]
    fn compact(&mut self) {
        if self.head == 0 {
            return;
        }
        let len = (self.tail - self.head) as usize;
        if len != 0 {
            // SAFETY: `head..tail` is initialized and lies inside `buf`;
            // `ptr::copy` permits the overlapping move to the prefix.
            unsafe {
                use std::ptr;
                ptr::copy(
                    self.buf.as_ptr().add(self.head as usize),
                    self.buf.as_mut_ptr(),
                    len,
                )
            };
        }
        self.head = 0;
        self.tail = len as u32;
    }
}

impl<const CAP: usize> buffer::PrefixLength for Inline<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<const CAP: usize> buffer::PrefixConsumer for Inline<CAP> {
    fn consume_validated_prefix(&mut self, proof: buffer::PrefixProof) {
        self.consume_valid(proof.amount());
    }
}

pub struct Rolling<const CAP: usize> {
    buf: [u8; CAP],
    head: u32,
    tail: u32,
}

// SAFETY: all-zero is a valid `Rolling` — empty `[0u8; CAP]` body, head == tail == 0.
unsafe impl<const CAP: usize> crate::mem::ZeroValid for Rolling<CAP> {}

impl<const CAP: usize> Default for Rolling<CAP> {
    #[inline(always)]
    fn default() -> Self {
        const {
            assert!(CAP <= u32::MAX as usize, "buffer::Rolling CAP must fit u32");
        }
        Self {
            buf: [0u8; CAP],
            head: 0,
            tail: 0,
        }
    }
}

impl<const CAP: usize> Rolling<CAP> {
    #[inline(always)]
    pub fn new() -> Self {
        Self::default()
    }

    #[inline(always)]
    pub const fn capacity(&self) -> usize {
        CAP
    }

    #[inline(always)]
    pub fn len(&self) -> usize {
        (self.tail - self.head) as usize
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.head == self.tail
    }

    #[inline(always)]
    pub fn spare_capacity(&self) -> usize {
        CAP - self.len()
    }

    #[inline(always)]
    pub fn as_slice(&self) -> &[u8] {
        let h = self.head as usize;
        let t = self.tail as usize;
        unsafe { self.buf.get_unchecked(h..t) }
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let h = self.head as usize;
        let t = self.tail as usize;
        unsafe { self.buf.get_unchecked_mut(h..t) }
    }

    pub fn spare_capacity_mut(&mut self) -> &mut [u8] {
        if (CAP - self.tail as usize) < self.spare_capacity() {
            self.compact();
        }
        let t = self.tail as usize;
        unsafe { self.buf.get_unchecked_mut(t..CAP) }
    }

    #[inline]
    pub unsafe fn advance(&mut self, n: usize) {
        let new_tail = self.tail as usize + n;
        debug_assert!(new_tail <= CAP, "buffer::Rolling::advance past CAP");
        self.tail = new_tail as u32;
    }

    pub fn push(&mut self, src: &[u8]) {
        let need = src.len();
        if need == 0 {
            return;
        }
        let tail_room = CAP - self.tail as usize;
        if need > tail_room {
            self.compact();
        }
        let tail = self.tail as usize;
        let avail = CAP - tail;
        assert!(
            need <= avail,
            "buffer::Rolling push overflow: need={need} cap={CAP}"
        );
        unsafe {
            std::ptr::copy_nonoverlapping(src.as_ptr(), self.buf.as_mut_ptr().add(tail), need);
        }
        self.tail = (tail + need) as u32;
    }

    pub fn consume(&mut self, n: usize) {
        let len = self.len();
        let n = n.min(len);
        self.head += n as u32;
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
        let h = self.head as usize;
        let t = self.tail as usize;
        let n = t - h;
        if n > 0 {
            unsafe {
                std::ptr::copy(self.buf.as_ptr().add(h), self.buf.as_mut_ptr(), n);
            }
        }
        self.head = 0;
        self.tail = n as u32;
    }
}

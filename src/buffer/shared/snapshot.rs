use super::super::storage::{RawMut, RawSpan};
use super::super::{CapacityError, PrefixLength};
use super::Shared;

pub struct SnapshotBuf<const MAX_CAPACITY: usize> {
    buf: RawMut,
    cap: u32,
    head: u32,
    tail: u32,
}

impl<const MAX_CAPACITY: usize> SnapshotBuf<MAX_CAPACITY> {
    const VALID: () = assert!(
        MAX_CAPACITY <= u32::MAX as usize,
        "SnapshotBuf MAX_CAPACITY must fit u32"
    );

    #[must_use]
    pub fn new() -> Self {
        let () = Self::VALID;
        Self {
            buf: RawMut::with_capacity_u32(0),
            cap: 0,
            head: 0,
            tail: 0,
        }
    }

    pub fn try_with_capacity(capacity: usize) -> Result<Self, CapacityError> {
        let () = Self::VALID;
        if capacity > MAX_CAPACITY {
            return Err(CapacityError::new(capacity, MAX_CAPACITY));
        }
        Ok(Self::with_valid_capacity(capacity))
    }

    #[must_use]
    pub fn with_capacity_up_to(requested: usize) -> Self {
        let () = Self::VALID;
        Self::with_valid_capacity(requested.min(MAX_CAPACITY))
    }

    fn with_valid_capacity(capacity: usize) -> Self {
        Self {
            buf: RawMut::with_capacity_u32(capacity as u32),
            cap: capacity as u32,
            head: 0,
            tail: 0,
        }
    }

    fn append(&mut self, src: &[u8]) {
        let n = src.len();
        if n > 0 {
            let head = self.head as usize;
            unsafe { self.buf.copy_from_slice_disjoint(head, src) };
            self.head = (head + n) as u32;
        }
    }

    #[cold]
    fn grow(&mut self, required: usize) {
        let mut new_cap = (self.cap as usize).max(1);
        while new_cap < required {
            new_cap = new_cap.saturating_mul(2).min(MAX_CAPACITY);
        }
        self.realloc(new_cap);
    }

    fn required(&self, additional: usize) -> Result<usize, CapacityError> {
        let required = self
            .len()
            .checked_add(additional)
            .ok_or_else(|| CapacityError::new(usize::MAX, MAX_CAPACITY))?;
        if required > MAX_CAPACITY {
            return Err(CapacityError::new(required, MAX_CAPACITY));
        }
        Ok(required)
    }

    fn realloc(&mut self, new_cap: usize) {
        let unparsed = (self.head - self.tail) as usize;
        let mut fresh = RawMut::with_capacity_u32(new_cap as u32);
        if unparsed > 0 {
            fresh.copy_from_raw(0, &self.buf, self.tail as usize, unparsed);
        }
        self.buf = fresh;
        self.cap = new_cap as u32;
        self.head = unparsed as u32;
        self.tail = 0;
    }

    pub fn try_extend_from_slice(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        if (self.cap - self.head) as usize >= src.len() {
            self.append(src);
            return Ok(());
        }
        let required = self.required(src.len())?;
        if required > self.cap as usize {
            self.grow(required);
        } else {
            self.compact();
            if ((self.cap - self.head) as usize) < src.len() {
                self.realloc(self.cap as usize);
            }
        }
        self.append(src);
        Ok(())
    }

    pub fn try_reserve_to(&mut self, target: usize) -> Result<(), CapacityError> {
        if target > MAX_CAPACITY {
            return Err(CapacityError::new(target, MAX_CAPACITY));
        }
        if (self.cap as usize) >= target {
            return Ok(());
        }
        self.realloc(target);
        Ok(())
    }

    pub fn snapshot(&self) -> Option<Shared> {
        let t = self.tail;
        let h = self.head;
        if h <= t {
            return None;
        }
        // SAFETY: SnapshotBuf maintains tail <= head <= buf.capacity().
        let span = unsafe { RawSpan::new_unchecked(self.buf.share(), t, h - t) };
        Some(Shared::from_raw_span(span))
    }

    pub fn is_empty(&self) -> bool {
        self.head <= self.tail
    }

    pub fn len(&self) -> usize {
        (self.head - self.tail) as usize
    }

    pub fn try_advance(&mut self, n: usize) -> bool {
        if n > self.len() {
            return false;
        }
        self.consume_valid(n);
        true
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        self.tail += amount as u32;
    }

    super::super::prefix::consume_prefix_api!(Self::consume_valid);

    pub fn compact(&mut self) {
        let t = self.tail as usize;
        let h = self.head as usize;
        if t == 0 {
            return;
        }
        if t >= h {
            if self.buf.is_unique() {
                self.head = 0;
                self.tail = 0;
            }
            return;
        }
        let unparsed = h - t;
        if !self.buf.detach_range(t..h, 0) {
            self.buf.copy_within(t..h, 0);
        }
        self.head = unparsed as u32;
        self.tail = 0;
    }
}

impl<const MAX_CAPACITY: usize> Default for SnapshotBuf<MAX_CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CAPACITY: usize> PrefixLength for SnapshotBuf<MAX_CAPACITY> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

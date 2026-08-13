use crate::buffer::{self, resident, storage};

trait Mode<P> {
    fn allocate(
        current: &storage::raw::AllocationMut<P>,
        capacity: u32,
    ) -> Result<storage::raw::AllocationMut<P>, buffer::CapacityError>;

    fn grow_unique(
        current: &mut storage::raw::AllocationMut<P>,
        capacity: u32,
    ) -> Result<(), buffer::CapacityError>;
}

struct Plain;
struct Accounted;

impl Mode<()> for Plain {
    fn allocate(
        _current: &storage::raw::AllocationMut<()>,
        capacity: u32,
    ) -> Result<storage::raw::AllocationMut<()>, buffer::CapacityError> {
        Ok(storage::raw::AllocationMut::with_capacity_u32(capacity))
    }

    fn grow_unique(
        current: &mut storage::raw::AllocationMut<()>,
        capacity: u32,
    ) -> Result<(), buffer::CapacityError> {
        current.grow_unique(capacity);
        Ok(())
    }
}

impl Mode<resident::Lease> for Accounted {
    fn allocate(
        current: &storage::raw::AllocationMut<resident::Lease>,
        capacity: u32,
    ) -> Result<storage::raw::AllocationMut<resident::Lease>, buffer::CapacityError> {
        current.sibling(capacity)
    }

    fn grow_unique(
        current: &mut storage::raw::AllocationMut<resident::Lease>,
        capacity: u32,
    ) -> Result<(), buffer::CapacityError> {
        current.grow_unique(capacity)
    }
}

pub(in crate::buffer) struct Raw<P, const MAX_CAPACITY: usize> {
    buf: storage::raw::AllocationMut<P>,
    cap: u32,
    head: u32,
    tail: u32,
}

struct Growth<'a, P, const MAX_CAPACITY: usize> {
    raw: &'a mut Raw<P, MAX_CAPACITY>,
}

impl<P, const MAX_CAPACITY: usize> Raw<P, MAX_CAPACITY> {
    const VALID: () = assert!(
        MAX_CAPACITY <= u32::MAX as usize,
        "Snapshot MAX_CAPACITY must fit u32"
    );

    pub(in crate::buffer) fn validate() {
        let () = Self::VALID;
    }

    pub(in crate::buffer) fn new(buf: storage::raw::AllocationMut<P>) -> Self {
        let () = Self::VALID;
        Self {
            buf,
            cap: 0,
            head: 0,
            tail: 0,
        }
    }

    pub(in crate::buffer) fn with_capacity(
        buf: storage::raw::AllocationMut<P>,
        capacity: usize,
    ) -> Self {
        let () = Self::VALID;
        Self {
            buf,
            cap: capacity as u32,
            head: 0,
            tail: 0,
        }
    }

    pub(in crate::buffer) fn span(&self) -> Option<storage::raw::Span<P>> {
        let tail = self.tail;
        let head = self.head;
        if head <= tail {
            return None;
        }
        Some(unsafe { storage::raw::Span::new_unchecked(self.buf.share(), tail, head - tail) })
    }

    pub(in crate::buffer) fn is_empty(&self) -> bool {
        self.head <= self.tail
    }

    pub(in crate::buffer) fn len(&self) -> usize {
        (self.head - self.tail) as usize
    }

    pub(in crate::buffer) fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        self.tail += amount as u32;
    }
}

impl<P, const MAX_CAPACITY: usize> Growth<'_, P, MAX_CAPACITY> {
    fn append(&mut self, src: &[u8]) {
        let len = src.len();
        if len > 0 {
            let head = self.raw.head as usize;
            unsafe { self.raw.buf.bytes_mut().copy_from_slice_disjoint(head, src) };
            self.raw.head = (head + len) as u32;
        }
    }

    fn required(&self, additional: usize) -> Result<usize, buffer::CapacityError> {
        let required = self
            .raw
            .len()
            .checked_add(additional)
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, MAX_CAPACITY))?;
        if required > MAX_CAPACITY {
            return Err(buffer::CapacityError::new(required, MAX_CAPACITY));
        }
        Ok(required)
    }

    fn realloc<M: Mode<P>>(&mut self, new_cap: usize) -> Result<(), buffer::CapacityError> {
        let raw = &mut self.raw;
        let unparsed = (raw.head - raw.tail) as usize;
        if raw.buf.is_unique() {
            M::grow_unique(&mut raw.buf, new_cap as u32)?;
            if raw.tail > 0 && unparsed > 0 {
                raw.buf
                    .bytes_mut()
                    .copy_within(raw.tail as usize..raw.head as usize, 0);
            }
        } else {
            let mut fresh = M::allocate(&raw.buf, new_cap as u32)?;
            if unparsed > 0 {
                fresh
                    .bytes_mut()
                    .copy_from_allocation(0, &raw.buf, raw.tail as usize, unparsed);
            }
            raw.buf = fresh;
        }
        raw.cap = new_cap as u32;
        raw.head = unparsed as u32;
        raw.tail = 0;
        Ok(())
    }

    fn grow<M: Mode<P>>(&mut self, required: usize) -> Result<(), buffer::CapacityError> {
        let mut new_cap = (self.raw.cap as usize).max(1);
        while new_cap < required {
            new_cap = new_cap.saturating_mul(2).min(MAX_CAPACITY);
        }
        match self.realloc::<M>(new_cap) {
            Ok(()) => Ok(()),
            Err(_) if new_cap > required => self.realloc::<M>(required),
            Err(error) => Err(error),
        }
    }

    fn try_extend<M: Mode<P>>(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        if (self.raw.cap - self.raw.head) as usize >= src.len() {
            self.append(src);
            return Ok(());
        }
        let required = self.required(src.len())?;
        if required > self.raw.cap as usize {
            self.grow::<M>(required)?;
        } else {
            self.compact::<M>()?;
            if ((self.raw.cap - self.raw.head) as usize) < src.len() {
                self.realloc::<M>(self.raw.cap as usize)?;
            }
        }
        self.append(src);
        Ok(())
    }

    fn try_reserve_to<M: Mode<P>>(&mut self, target: usize) -> Result<(), buffer::CapacityError> {
        if target > MAX_CAPACITY {
            return Err(buffer::CapacityError::new(target, MAX_CAPACITY));
        }
        if (self.raw.cap as usize) >= target {
            return Ok(());
        }
        self.realloc::<M>(target)
    }

    fn compact<M: Mode<P>>(&mut self) -> Result<(), buffer::CapacityError> {
        let tail = self.raw.tail as usize;
        let head = self.raw.head as usize;
        if tail == 0 {
            return Ok(());
        }
        if tail >= head {
            if self.raw.buf.is_unique() {
                self.raw.head = 0;
                self.raw.tail = 0;
            }
            return Ok(());
        }
        let unparsed = head - tail;
        if self.raw.buf.is_unique() {
            self.raw.buf.bytes_mut().copy_within(tail..head, 0);
        } else {
            let mut fresh = M::allocate(&self.raw.buf, self.raw.cap)?;
            fresh
                .bytes_mut()
                .copy_from_allocation(0, &self.raw.buf, tail, unparsed);
            self.raw.buf = fresh;
        }
        self.raw.head = unparsed as u32;
        self.raw.tail = 0;
        Ok(())
    }
}

pub struct Snapshot<const MAX_CAPACITY: usize> {
    raw: Raw<(), MAX_CAPACITY>,
}

impl<const MAX_CAPACITY: usize> Snapshot<MAX_CAPACITY> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            raw: Raw::new(storage::raw::AllocationMut::with_capacity_u32(0)),
        }
    }

    #[must_use]
    pub fn with_capacity_up_to(requested: usize) -> Self {
        Raw::<(), MAX_CAPACITY>::validate();
        let capacity = requested.min(MAX_CAPACITY);
        Self {
            raw: Raw::with_capacity(
                storage::raw::AllocationMut::with_capacity_u32(capacity as u32),
                capacity,
            ),
        }
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        self.raw.try_extend(src)
    }

    pub fn try_reserve_to(&mut self, target: usize) -> Result<(), buffer::CapacityError> {
        self.raw.try_reserve_to(target)
    }

    pub fn snapshot(&self) -> Option<storage::Shared> {
        self.raw.span().map(storage::Shared::from_span)
    }

    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    pub fn compact(&mut self) {
        let result = self.raw.compact();
        debug_assert!(result.is_ok());
    }
}

impl<const MAX_CAPACITY: usize> Raw<(), MAX_CAPACITY> {
    fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        Growth { raw: self }.try_extend::<Plain>(src)
    }

    fn try_reserve_to(&mut self, target: usize) -> Result<(), buffer::CapacityError> {
        Growth { raw: self }.try_reserve_to::<Plain>(target)
    }

    fn compact(&mut self) -> Result<(), buffer::CapacityError> {
        Growth { raw: self }.compact::<Plain>()
    }
}

impl<const MAX_CAPACITY: usize> Raw<resident::Lease, MAX_CAPACITY> {
    pub(in crate::buffer) fn new_accounted(budget: &resident::Budget<'_>) -> Self {
        Self::new(storage::raw::AllocationMut::with_budget_zero(budget))
    }

    pub(in crate::buffer) fn with_accounted_capacity(
        budget: &resident::Budget<'_>,
        capacity: usize,
    ) -> Result<Self, buffer::CapacityError> {
        let allocation = storage::raw::AllocationMut::with_budget(capacity as u32, budget)?;
        Ok(Self::with_capacity(allocation, capacity))
    }

    pub(in crate::buffer) fn try_extend(
        &mut self,
        src: &[u8],
    ) -> Result<(), buffer::CapacityError> {
        Growth { raw: self }.try_extend::<Accounted>(src)
    }

    pub(in crate::buffer) fn try_reserve_to(
        &mut self,
        target: usize,
    ) -> Result<(), buffer::CapacityError> {
        Growth { raw: self }.try_reserve_to::<Accounted>(target)
    }

    pub(in crate::buffer) fn compact(&mut self) -> Result<(), buffer::CapacityError> {
        Growth { raw: self }.compact::<Accounted>()
    }

    pub(in crate::buffer) fn release_empty(&mut self) {
        if !self.is_empty() {
            std::process::abort();
        }
        let fresh = match self.buf.sibling(0) {
            Ok(fresh) => fresh,
            Err(_) => std::process::abort(),
        };
        self.buf = fresh;
        self.cap = 0;
        self.head = 0;
        self.tail = 0;
    }
}

impl<const MAX_CAPACITY: usize> Default for Snapshot<MAX_CAPACITY> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const MAX_CAPACITY: usize> buffer::PrefixLength for Snapshot<MAX_CAPACITY> {
    fn prefix_len(&self) -> usize {
        self.raw.len()
    }
}

impl<const MAX_CAPACITY: usize> buffer::PrefixConsumer for Snapshot<MAX_CAPACITY> {
    fn consume_validated_prefix(&mut self, proof: buffer::PrefixProof) {
        self.raw.consume_valid(proof.amount());
    }
}

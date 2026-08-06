use crate::buffer::{
    Bytes, CapacityError, PrefixConsumer, PrefixLength, PrefixProof, Retained, SpareWriter,
    pool::{Lease, PoolCapacity, RuntimePoolCapacity, Uninitialized},
};

/// A pooled byte cursor over one logical readable range.
pub struct Cursor<C: PoolCapacity = RuntimePoolCapacity> {
    pub(super) lease: Lease<Uninitialized, C>,
    pub(super) head: u32,
}

impl<C: PoolCapacity> Cursor<C> {
    pub(super) const fn new(lease: Lease<Uninitialized, C>) -> Self {
        Self { lease, head: 0 }
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.lease.len() - self.head as usize
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        if self.head == 0 {
            self.lease.is_empty()
        } else {
            self.head as usize == self.lease.len()
        }
    }

    #[must_use]
    pub fn spare_capacity(&self) -> usize {
        self.lease.capacity() - self.len()
    }

    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.lease.as_slice()[self.head as usize..]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let head = self.head as usize;
        &mut self.lease.as_mut_slice()[head..]
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), CapacityError> {
        let capacity = self.lease.capacity();
        let lease_len = self.lease.len();
        if src.len() > capacity - lease_len
            && src.len() <= capacity - (lease_len - self.head as usize)
        {
            self.compact();
        }
        self.lease.try_extend(src)
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        if self.lease.len() == self.lease.capacity() && self.head != 0 {
            self.compact();
        }
        self.lease.try_push(byte)
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), CapacityError> {
        let additional = slices.iter().try_fold(0usize, |len, slice| {
            len.checked_add(slice.len())
                .ok_or_else(|| CapacityError::new(usize::MAX, self.lease.capacity()))
        })?;
        if additional > self.lease.capacity() - self.lease.len()
            && additional <= self.spare_capacity()
        {
            self.compact();
        }
        self.lease.try_extend_from_slices(slices)
    }

    pub fn truncate(&mut self, len: usize) {
        let len = len.min(self.len());
        self.lease.truncate(self.head as usize + len);
        if len == 0 {
            self.head = 0;
            self.lease.truncate(0);
        }
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.as_slice().as_ptr()
    }

    /// Returns a contiguous writer after compacting the readable range.
    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        self.compact();
        self.lease.spare_writer()
    }

    #[must_use]
    pub fn freeze(self) -> Bytes<Retained> {
        let head = self.head as usize;
        let mut bytes = Bytes::<Retained>::from(self.lease.freeze());
        bytes.consume_prefix_up_to(head);
        bytes
    }

    fn compact(&mut self) {
        if self.head == 0 {
            return;
        }
        let len = self.len();
        self.lease
            .as_mut_slice()
            .copy_within(self.head as usize.., 0);
        self.head = 0;
        self.lease.truncate(len);
    }

    fn consume_valid(&mut self, amount: usize) {
        debug_assert!(amount <= self.len());
        self.head += amount as u32;
        if self.head as usize == self.lease.len() {
            self.head = 0;
            self.lease.truncate(0);
        }
    }
}

impl<C: PoolCapacity> AsRef<[u8]> for Cursor<C> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<C: PoolCapacity> PrefixLength for Cursor<C> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<C: PoolCapacity> PrefixConsumer for Cursor<C> {
    fn consume_validated_prefix(&mut self, proof: PrefixProof) {
        self.consume_valid(proof.amount());
    }
}

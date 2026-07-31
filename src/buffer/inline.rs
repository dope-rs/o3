use std::fmt;
use std::ops::Deref;

use super::{CapacityError, PrefixLength};

pub const INLINE_BYTES_CAPACITY: usize = 24;

#[derive(Clone, PartialEq, Eq)]
pub struct InlineBytes<const CAP: usize = INLINE_BYTES_CAPACITY> {
    bytes: [u8; CAP],
    len: u8,
}

impl<const CAP: usize> InlineBytes<CAP> {
    const VALID: () = assert!(
        CAP <= u8::MAX as usize,
        "buffer::InlineBytes CAP must fit u8"
    );

    pub const CAPACITY: usize = CAP;

    #[must_use]
    pub const fn new() -> Self {
        let () = Self::VALID;
        Self {
            bytes: [0; CAP],
            len: 0,
        }
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, CapacityError> {
        let mut inline = Self::new();
        inline.try_extend_from_slice(bytes)?;
        Ok(inline)
    }

    pub fn try_extend_from_slice(&mut self, bytes: &[u8]) -> Result<(), CapacityError> {
        let start = usize::from(self.len);
        let end = start + bytes.len();
        if end > CAP {
            return Err(CapacityError::new(end, CAP));
        }
        self.bytes[start..end].copy_from_slice(bytes);
        self.len = end as u8;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), CapacityError> {
        let len = usize::from(self.len);
        if len == CAP {
            return Err(CapacityError::new(len + 1, CAP));
        }
        self.bytes[len] = byte;
        self.len += 1;
        Ok(())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const CAP: usize> Default for InlineBytes<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> AsRef<[u8]> for InlineBytes<CAP> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const CAP: usize> PrefixLength for InlineBytes<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<const CAP: usize> Deref for InlineBytes<CAP> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const CAP: usize> fmt::Debug for InlineBytes<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InlineBytes")
            .field("bytes", &self.bytes)
            .field("len", &self.len)
            .finish()
    }
}

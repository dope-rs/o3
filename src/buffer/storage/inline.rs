use std::{fmt, ops::Deref};

use crate::buffer;

pub const CAPACITY: usize = 24;

#[derive(Clone, PartialEq, Eq)]
pub struct Bytes<const CAP: usize = CAPACITY> {
    bytes: [u8; CAP],
    len: u8,
}

impl<const CAP: usize> Bytes<CAP> {
    const VALID: () = assert!(
        CAP <= u8::MAX as usize,
        "buffer::storage::inline::Bytes CAP must fit u8"
    );

    #[must_use]
    pub const fn new() -> Self {
        let () = Self::VALID;
        Self {
            bytes: [0; CAP],
            len: 0,
        }
    }

    pub fn from_slice(bytes: &[u8]) -> Result<Self, buffer::CapacityError> {
        let mut inline = Self::new();
        inline.try_extend(bytes)?;
        Ok(inline)
    }

    pub fn try_extend(&mut self, bytes: &[u8]) -> Result<(), buffer::CapacityError> {
        let start = usize::from(self.len);
        let end = start + bytes.len();
        if end > CAP {
            return Err(buffer::CapacityError::new(end, CAP));
        }
        self.bytes[start..end].copy_from_slice(bytes);
        self.len = end as u8;
        Ok(())
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), buffer::CapacityError> {
        let len = usize::from(self.len);
        if len == CAP {
            return Err(buffer::CapacityError::new(len + 1, CAP));
        }
        self.bytes[len] = byte;
        self.len += 1;
        Ok(())
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }

    const fn len(&self) -> usize {
        self.len as usize
    }
}

impl<const CAP: usize> Default for Bytes<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> AsRef<[u8]> for Bytes<CAP> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const CAP: usize> buffer::PrefixLength for Bytes<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<const CAP: usize> Deref for Bytes<CAP> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const CAP: usize> fmt::Debug for Bytes<CAP> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bytes")
            .field("bytes", &self.bytes)
            .field("len", &self.len)
            .finish()
    }
}

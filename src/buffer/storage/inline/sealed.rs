use std::{fmt, ops, str};

use crate::buffer;

#[repr(transparent)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Str<const CAP: usize> {
    text: Text<CAP>,
}

impl<const CAP: usize> Str<CAP> {
    pub const fn new() -> Self {
        Self { text: Text::new() }
    }

    pub fn from_str_truncated(value: &str) -> Self {
        Self {
            text: Text::from_str_truncated(value),
        }
    }

    pub fn as_str(&self) -> &str {
        self.text.as_str()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

impl<const CAP: usize> Default for Str<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> AsRef<str> for Str<CAP> {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl<const CAP: usize> fmt::Debug for Str<CAP> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(formatter)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Text<const CAP: usize> {
    bytes: [u8; CAP],
    len: usize,
}

impl<const CAP: usize> Text<CAP> {
    const fn new() -> Self {
        Self {
            bytes: [0; CAP],
            len: 0,
        }
    }

    fn from_str_truncated(value: &str) -> Self {
        let mut len = value.len().min(CAP);
        while !value.is_char_boundary(len) {
            len -= 1;
        }
        let mut text = Self::new();
        text.bytes[..len].copy_from_slice(&value.as_bytes()[..len]);
        text.len = len;
        text
    }

    fn as_str(&self) -> &str {
        // SAFETY: construction copies a prefix ending at a source string's
        // character boundary and no API exposes mutable byte access.
        unsafe { str::from_utf8_unchecked(&self.bytes[..self.len]) }
    }

    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct Bytes<const CAP: usize = { super::CAPACITY }> {
    bytes: [u8; CAP],
    len: u8,
}

/// Fixed-capacity owned bytes with a length capable of addressing a `u16`
/// sized allocation.
#[derive(Clone, PartialEq, Eq)]
pub struct WideBytes<const CAP: usize> {
    bytes: [u8; CAP],
    len: u16,
}

impl<const CAP: usize> WideBytes<CAP> {
    const VALID: () = assert!(
        CAP <= u16::MAX as usize,
        "buffer::storage::inline::WideBytes CAP must fit u16"
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
        let Some(end) = start.checked_add(bytes.len()) else {
            return Err(buffer::CapacityError::new(usize::MAX, CAP));
        };
        if end > CAP {
            return Err(buffer::CapacityError::new(end, CAP));
        }
        self.bytes[start..end].copy_from_slice(bytes);
        self.len = end as u16;
        Ok(())
    }

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        let end = buffer::checked_append_len(usize::from(self.len), CAP, &slices)?;
        let mut offset = usize::from(self.len);
        for slice in slices {
            let next = offset + slice.len();
            self.bytes[offset..next].copy_from_slice(slice);
            offset = next;
        }
        self.len = end as u16;
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

    pub const fn len(&self) -> usize {
        self.len as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl<const CAP: usize> Default for WideBytes<CAP> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const CAP: usize> AsRef<[u8]> for WideBytes<CAP> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<const CAP: usize> buffer::PrefixLength for WideBytes<CAP> {
    fn prefix_len(&self) -> usize {
        self.len()
    }
}

impl<const CAP: usize> ops::Deref for WideBytes<CAP> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<const CAP: usize> fmt::Debug for WideBytes<CAP> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WideBytes")
            .field("bytes", &self.bytes)
            .field("len", &self.len)
            .finish()
    }
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

    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        let end = buffer::checked_append_len(usize::from(self.len), CAP, &slices)?;
        let mut offset = usize::from(self.len);
        for slice in slices {
            let next = offset + slice.len();
            self.bytes[offset..next].copy_from_slice(slice);
            offset = next;
        }
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

impl<const CAP: usize> ops::Deref for Bytes<CAP> {
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

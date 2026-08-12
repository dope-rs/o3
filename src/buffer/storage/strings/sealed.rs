use std::{fmt, ops, str};

use crate::buffer::storage;

#[repr(transparent)]
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct Str(storage::Shared);

impl Str {
    pub const fn new() -> Self {
        Self(storage::Shared::new())
    }

    pub const fn from_static(value: &'static str) -> Self {
        Self(storage::Shared::from_static(value.as_bytes()))
    }

    pub fn from_utf8(value: storage::Shared) -> Result<Self, str::Utf8Error> {
        str::from_utf8(value.as_slice())?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        unsafe {
            use std::str::from_utf8_unchecked;
            from_utf8_unchecked(self.0.as_slice())
        }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }
}

impl Default for Str {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<storage::Shared> for Str {
    type Error = str::Utf8Error;

    fn try_from(value: storage::Shared) -> Result<Self, Self::Error> {
        Self::from_utf8(value)
    }
}

impl From<String> for Str {
    fn from(value: String) -> Self {
        Self(storage::Shared::from(value))
    }
}

impl From<&str> for Str {
    fn from(value: &str) -> Self {
        Self(storage::Shared::from(value))
    }
}

impl AsRef<str> for Str {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for Str {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl ops::Deref for Str {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Str {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

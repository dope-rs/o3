use std::fmt;
use std::ops::Deref;
use std::str::Utf8Error;

use super::Shared;

#[repr(transparent)]
#[derive(Clone, Eq, Hash, PartialEq)]
pub struct SharedStr(Shared);

impl SharedStr {
    pub const fn new() -> Self {
        Self(Shared::new())
    }

    pub const fn from_static(value: &'static str) -> Self {
        Self(Shared::from_static(value.as_bytes()))
    }

    pub fn from_utf8(value: Shared) -> Result<Self, Utf8Error> {
        std::str::from_utf8(value.as_slice())?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        unsafe { std::str::from_utf8_unchecked(self.0.as_slice()) }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_slice()
    }

    pub fn into_shared(self) -> Shared {
        self.0
    }
}

impl Default for SharedStr {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<Shared> for SharedStr {
    type Error = Utf8Error;

    fn try_from(value: Shared) -> Result<Self, Self::Error> {
        Self::from_utf8(value)
    }
}

impl From<String> for SharedStr {
    fn from(value: String) -> Self {
        Self(Shared::from(value))
    }
}

impl From<&str> for SharedStr {
    fn from(value: &str) -> Self {
        Self(Shared::from(value))
    }
}

impl AsRef<str> for SharedStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<[u8]> for SharedStr {
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Deref for SharedStr {
    type Target = str;

    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for SharedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for SharedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

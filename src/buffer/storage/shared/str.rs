use std::{ops, str};

use crate::{
    buffer,
    buffer::{storage, storage::regions},
};

/// An immutable owner whose complete logical range is valid UTF-8.
#[repr(transparent)]
pub struct Str<O: buffer::raw::Source = storage::Shared> {
    text: buffer::raw::Utf8<O>,
}

impl<O: buffer::raw::Source> Str<O> {
    pub fn from_utf8(owner: O) -> Result<Self, str::Utf8Error> {
        Ok(Self {
            text: buffer::raw::Utf8::try_new(owner)?,
        })
    }
}

impl<O: buffer::raw::Source> ops::Deref for Str<O> {
    type Target = buffer::raw::Utf8<O>;
    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

impl TryFrom<&mut String> for Str {
    type Error = crate::alloc::Exhausted;

    /// Transfers the text on success; allocation failure leaves it unchanged.
    fn try_from(value: &mut String) -> Result<Self, Self::Error> {
        let owner = storage::Shared {
            region: regions::Header::try_new(value)?,
        };
        // SAFETY: String transfers its complete validated UTF-8 bytes unchanged.
        Ok(Self {
            text: unsafe { buffer::raw::Utf8::new(owner) },
        })
    }
}

const _: () = {
    assert!(size_of::<Str>() == size_of::<storage::Shared>());
    assert!(size_of::<Result<Str, crate::alloc::Exhausted>>() == size_of::<Str>());
};

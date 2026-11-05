use std::str;

use crate::{
    buffer,
    buffer::{resident, storage, storage::regions},
    mem::rc,
};

/// An immutable owner whose complete logical range is valid UTF-8.
#[derive(Debug)]
#[repr(transparent)]
pub struct Str<O: buffer::raw::Source = storage::Shared> {
    owner: O,
}

impl<O: buffer::raw::Source> Str<O> {
    pub fn from_utf8(owner: O) -> Result<Self, str::Utf8Error> {
        str::from_utf8(owner.as_ref())?;
        Ok(Self { owner })
    }

    /// # Safety
    /// The source's entire unchanged visible range is valid UTF-8.
    pub(in crate::buffer) unsafe fn new(owner: O) -> Self {
        Self { owner }
    }

    pub fn as_str(&self) -> &str {
        // SAFETY: construction proves UTF-8 and Source preserves the complete range.
        unsafe { str::from_utf8_unchecked(self.owner.as_ref()) }
    }

    pub fn as_bytes(&self) -> &[u8] {
        self.owner.as_ref()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.owner.as_ref().len()
    }
}

impl Str<resident::Exact<'_>> {
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            owner: self.owner.try_clone()?,
        })
    }
}

impl TryFrom<&mut String> for Str {
    type Error = crate::alloc::Exhausted;

    /// Transfers the text on success; allocation failure leaves it unchanged.
    fn try_from(value: &mut String) -> Result<Self, Self::Error> {
        let owner = regions::Header::try_new(value)?;
        Ok(Self { owner })
    }
}

const _: () = {
    assert!(size_of::<Str>() == size_of::<storage::Shared>());
    assert!(size_of::<Result<Str, crate::alloc::Exhausted>>() == size_of::<Str>());
    assert!(size_of::<Str<resident::Exact<'static>>>() == size_of::<usize>());
};

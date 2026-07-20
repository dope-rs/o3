use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::num::{NonZeroU32, NonZeroU64};

use crate::collections::IndexKey;
use crate::collections::index;
use crate::marker::ThreadBound;

use super::GenerationState;

#[repr(transparent)]
pub struct SlabKey<Tag = (), const MAX: u32 = { u32::MAX }> {
    parts: SlabKeyParts<MAX>,
    marker: PhantomData<*mut Tag>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SlabKeyParts<const MAX: u32 = { u32::MAX }> {
    raw: NonZeroU64,
    marker: PhantomData<*mut ()>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SlabGeneration<const MAX: u32 = { u32::MAX }>(NonZeroU32, ThreadBound);

impl<const MAX: u32> SlabGeneration<MAX> {
    const VALID: () = assert!(MAX != 0, "generation limit must be nonzero");
    pub const MIN: Self = {
        let () = Self::VALID;
        Self(NonZeroU32::MIN, ThreadBound::NEW)
    };

    #[must_use]
    pub const fn new(raw: u32) -> Option<Self> {
        let () = Self::VALID;
        match NonZeroU32::new(raw) {
            Some(_) if raw > MAX => None,
            Some(raw) => Some(Self(raw, ThreadBound::NEW)),
            None => None,
        }
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    #[must_use]
    pub const fn checked_add(self, value: u32) -> Option<Self> {
        let () = Self::VALID;
        match self.0.checked_add(value) {
            Some(raw) if raw.get() <= MAX => Some(Self(raw, ThreadBound::NEW)),
            None => None,
            Some(_) => None,
        }
    }
}

impl<const MAX: u32> SlabKeyParts<MAX> {
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Option<Self> {
        match SlabGeneration::new(generation) {
            Some(generation) => Some(Self::from_generation(index, generation)),
            None => None,
        }
    }

    pub const fn from_generation(index: u32, generation: SlabGeneration<MAX>) -> Self {
        let raw = ((generation.get() as u64) << 32) | index as u64;
        Self {
            raw: unsafe { NonZeroU64::new_unchecked(raw) },
            marker: PhantomData,
        }
    }

    pub const fn index(self) -> u32 {
        self.raw.get() as u32
    }

    pub const fn generation(self) -> SlabGeneration<MAX> {
        SlabGeneration(
            unsafe { NonZeroU32::new_unchecked((self.raw.get() >> 32) as u32) },
            ThreadBound::NEW,
        )
    }
}

impl<const MAX: u32> fmt::Debug for SlabKeyParts<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlabKeyParts")
            .field("index", &self.index())
            .field("generation", &self.generation())
            .finish()
    }
}

impl<Tag, const MAX: u32> SlabKey<Tag, MAX> {
    pub(crate) const fn new(index: u32, generation: SlabGeneration<MAX>) -> Self {
        Self::from_parts(SlabKeyParts::from_generation(index, generation))
    }

    pub(crate) const fn from_parts(parts: SlabKeyParts<MAX>) -> Self {
        Self {
            parts,
            marker: PhantomData,
        }
    }

    pub const fn index(self) -> u32 {
        self.parts.index()
    }

    pub const fn generation(self) -> SlabGeneration<MAX> {
        self.parts.generation()
    }

    pub const fn parts(self) -> SlabKeyParts<MAX> {
        self.parts
    }
}

impl<Tag, const MAX: u32> From<SlabKey<Tag, MAX>> for SlabKeyParts<MAX> {
    fn from(key: SlabKey<Tag, MAX>) -> Self {
        key.parts
    }
}

impl<Tag, const MAX: u32> Clone for SlabKey<Tag, MAX> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, const MAX: u32> Copy for SlabKey<Tag, MAX> {}

impl<Tag, const MAX: u32> PartialEq for SlabKey<Tag, MAX> {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl<Tag, const MAX: u32> Eq for SlabKey<Tag, MAX> {}

impl<Tag, const MAX: u32> Hash for SlabKey<Tag, MAX> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts.hash(state);
    }
}

impl<Tag, const MAX: u32> fmt::Debug for SlabKey<Tag, MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlabKey")
            .field("index", &self.index())
            .field("generation", &self.generation())
            .finish()
    }
}

impl<const MAX: u32> fmt::Debug for SlabGeneration<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SlabGeneration").field(&self.get()).finish()
    }
}

impl<Tag, const MAX: u32> IndexKey for SlabKey<Tag, MAX> {
    fn index(self) -> usize {
        self.index() as usize
    }
}

impl<Tag, const MAX: u32> index::Sealed for SlabKey<Tag, MAX> {}

impl<const MAX: u32> GenerationState for SlabGeneration<MAX> {
    const MIN: Self = Self::MIN;
    const VALID: () = Self::VALID;

    fn next(self) -> Option<Self> {
        self.checked_add(1)
    }
}

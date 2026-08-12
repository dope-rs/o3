use std::{fmt, hash, marker, num};

use crate::collections::slab;

#[repr(transparent)]
pub struct Key<Tag = (), const MAX: u32 = { u32::MAX }> {
    parts: Parts<MAX>,
    marker: marker::PhantomData<*mut Tag>,
}

#[derive(Clone, Copy, PartialEq, Eq, hash::Hash)]
#[repr(transparent)]
pub struct Parts<const MAX: u32 = { u32::MAX }> {
    raw: num::NonZeroU64,
    marker: marker::PhantomData<*mut ()>,
}

#[derive(Clone, Copy, PartialEq, Eq, hash::Hash)]
#[repr(transparent)]
pub struct Generation<const MAX: u32 = { u32::MAX }>(num::NonZeroU32, crate::ThreadBound);

impl<const MAX: u32> Generation<MAX> {
    const THREAD_BOUND: crate::ThreadBound = {
        use crate::ThreadBound;
        ThreadBound::NEW
    };
    const VALID: () = assert!(MAX != 0, "generation limit must be nonzero");
    pub const MIN: Self = {
        let () = Self::VALID;
        Self(num::NonZeroU32::MIN, Self::THREAD_BOUND)
    };

    #[must_use]
    pub const fn new(raw: u32) -> Option<Self> {
        let () = Self::VALID;
        match num::NonZeroU32::new(raw) {
            Some(_) if raw > MAX => None,
            Some(raw) => Some(Self(raw, Self::THREAD_BOUND)),
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
            Some(raw) if raw.get() <= MAX => Some(Self(raw, Self::THREAD_BOUND)),
            None => None,
            Some(_) => None,
        }
    }
}

impl<const MAX: u32> Parts<MAX> {
    #[must_use]
    pub const fn new(index: u32, generation: u32) -> Option<Self> {
        match Generation::new(generation) {
            Some(generation) => Some(Self::from_generation(index, generation)),
            None => None,
        }
    }

    pub const fn from_generation(index: u32, generation: Generation<MAX>) -> Self {
        let raw = ((generation.get() as u64) << 32) | index as u64;
        Self {
            raw: unsafe { num::NonZeroU64::new_unchecked(raw) },
            marker: marker::PhantomData,
        }
    }

    pub const fn index(self) -> u32 {
        self.raw.get() as u32
    }

    pub const fn generation(self) -> Generation<MAX> {
        Generation(
            unsafe { num::NonZeroU32::new_unchecked((self.raw.get() >> 32) as u32) },
            Generation::<MAX>::THREAD_BOUND,
        )
    }
}

impl<const MAX: u32> fmt::Debug for Parts<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Parts")
            .field("index", &self.index())
            .field("generation", &self.generation())
            .finish()
    }
}

impl<Tag, const MAX: u32> Key<Tag, MAX> {
    pub(crate) const fn new(index: u32, generation: Generation<MAX>) -> Self {
        Self::from_parts(Parts::from_generation(index, generation))
    }

    pub(super) const fn from_parts(parts: Parts<MAX>) -> Self {
        Self {
            parts,
            marker: marker::PhantomData,
        }
    }

    pub const fn index(self) -> u32 {
        self.parts.index()
    }

    pub const fn generation(self) -> Generation<MAX> {
        self.parts.generation()
    }

    pub const fn parts(self) -> Parts<MAX> {
        self.parts
    }

    pub const fn retag<Other>(self) -> Key<Other, MAX> {
        Key {
            parts: self.parts,
            marker: marker::PhantomData,
        }
    }

    pub const fn with_generation(self, generation: Generation<MAX>) -> Self {
        Self::new(self.index(), generation)
    }
}

impl<Tag, const MAX: u32> From<Key<Tag, MAX>> for Parts<MAX> {
    fn from(key: Key<Tag, MAX>) -> Self {
        key.parts
    }
}

impl<Tag, const MAX: u32> Clone for Key<Tag, MAX> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, const MAX: u32> Copy for Key<Tag, MAX> {}

impl<Tag, const MAX: u32> PartialEq for Key<Tag, MAX> {
    fn eq(&self, other: &Self) -> bool {
        self.parts == other.parts
    }
}

impl<Tag, const MAX: u32> Eq for Key<Tag, MAX> {}

impl<Tag, const MAX: u32> hash::Hash for Key<Tag, MAX> {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.parts.hash(state);
    }
}

impl<Tag, const MAX: u32> fmt::Debug for Key<Tag, MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Key")
            .field("index", &self.index())
            .field("generation", &self.generation())
            .finish()
    }
}

impl<const MAX: u32> fmt::Debug for Generation<MAX> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Generation").field(&self.get()).finish()
    }
}

impl<const MAX: u32> slab::GenerationState for Generation<MAX> {
    const MIN: Self = Self::MIN;
    const VALID: () = Self::VALID;

    fn next(self) -> Option<Self> {
        self.checked_add(1)
    }
}

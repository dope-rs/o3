use std::num::NonZeroU32;
use std::num::NonZeroU64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(C)]
pub struct SlotId {
    id_raw: NonZeroU64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct SlotGen(NonZeroU32);

impl SlotGen {
    pub const MIN: Self = Self(NonZeroU32::MIN);

    #[must_use]
    pub const fn new(raw: u32) -> Option<Self> {
        match NonZeroU32::new(raw) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }

    pub const fn get(self) -> u32 {
        self.0.get()
    }

    #[must_use]
    pub const fn checked_add(self, other: u32) -> Option<Self> {
        match self.0.checked_add(other) {
            Some(raw) => Some(Self(raw)),
            None => None,
        }
    }
}

impl SlotId {
    #[must_use]
    pub const fn from_parts(slot: u32, generation: SlotGen) -> Self {
        let raw = ((generation.get() as u64) << 32) | (slot as u64);
        Self {
            id_raw: unsafe { NonZeroU64::new_unchecked(raw) },
        }
    }

    pub const fn slot(self) -> u32 {
        self.id_raw.get() as u32
    }

    pub const fn generation(self) -> SlotGen {
        SlotGen(unsafe { NonZeroU32::new_unchecked((self.id_raw.get() >> 32) as u32) })
    }
}

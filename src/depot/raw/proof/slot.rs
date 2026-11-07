use std::marker;

use crate::depot::index::Stored;

/// Vacancy-bearing storage word for one key domain.
#[repr(transparent)]
#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) struct Slot<K: crate::depot::raw::Key> {
    raw: K::Raw,
    marker: Identity<K>,
}

impl<K: crate::depot::raw::Key> Slot<K> {
    pub(crate) const fn vacant() -> Self {
        Self {
            raw: K::VACANT,
            marker: marker::PhantomData,
        }
    }

    pub(crate) fn from_key(key: K) -> Self {
        Self {
            raw: key.into_raw(),
            marker: marker::PhantomData,
        }
    }

    /// Reconstructs the live key from a caller-proven occupied slot.
    /// # Safety
    /// This slot must contain an identity stored from a live key in this `K` domain.
    pub(crate) unsafe fn live(self) -> K {
        // SAFETY: the caller proves this slot contains an identity emitted by K.
        let raw = unsafe { Stored::new(self.raw) };
        K::from_raw(raw)
    }

    pub(crate) fn is_vacant(self) -> bool {
        self.raw == K::VACANT
    }

    pub(crate) fn present(self) -> Option<K> {
        if self.is_vacant() {
            return None;
        }
        // SAFETY: construction stores either VACANT or an identity emitted by K.
        Some(unsafe { self.live() })
    }
}

type Identity<K> = marker::PhantomData<(K, fn(K) -> K)>;

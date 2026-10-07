use std::marker;

/// A raw identity proven emitted by a live key in this exact key domain.
#[repr(transparent)]
pub struct Stored<K: crate::depot::raw::Key> {
    raw: K::Raw,
    marker: Identity<K>,
}

impl<K: crate::depot::raw::Key> Stored<K> {
    /// # Safety
    /// `raw` must equal [`K::into_raw`] for a live key in this exact `K` domain.
    pub(in crate::depot) const unsafe fn new(raw: K::Raw) -> Self {
        Self {
            raw,
            marker: marker::PhantomData,
        }
    }

    pub fn into_raw(self) -> K::Raw {
        self.raw
    }
}

type Identity<K> = marker::PhantomData<(K, fn(K) -> K)>;

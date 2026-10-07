use crate::depot::queue::{indexed::keyed, links};

/// One exact stable link projected from caller-owned storage.
pub struct Entry<'a, K: crate::depot::raw::Key, P = ()> {
    index: links::Index,
    key: K,
    link: &'a keyed::Link<K, P>,
}

impl<K: crate::depot::raw::Key, P> Clone for Entry<'_, K, P> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: crate::depot::raw::Key, P> Copy for Entry<'_, K, P> {}

impl<'a, K: crate::depot::raw::Key, P> Entry<'a, K, P> {
    /// Projects a retained native source.
    pub fn from_source<S: crate::depot::raw::KeyBorrow<'a, P, Key = K>>(source: S) -> Self {
        let (key, link) = source.parts();
        // SAFETY: Projection retains this exact bounded key and its co-located link.
        let index = unsafe { links::Index::new_raw(key.index() as u32) };
        Self { index, key, link }
    }

    pub(crate) const fn index(self) -> links::Index {
        self.index
    }

    pub(crate) const fn key(self) -> K {
        self.key
    }

    pub(crate) const fn link(self) -> &'a keyed::Link<K, P> {
        self.link
    }

    pub(crate) fn predecessor(self) -> Option<links::Index> {
        self.link.raw.prev_index()
    }

    pub(crate) fn successor(self) -> Option<links::Index> {
        self.link.raw.next_index()
    }
}

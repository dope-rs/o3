use std::ops::Range;

use super::{Pooled, Shared};

#[derive(Clone)]
enum Repr<'a> {
    Borrowed(&'a [u8]),
    Pooled(Pooled),
    Shared(Shared),
}

#[derive(Clone)]
pub struct View<'a> {
    repr: Repr<'a>,
    range: Range<usize>,
}

enum OwnedRepr {
    Pooled {
        pooled: Pooled,
        start: u32,
        len: u32,
    },
    Shared(Shared),
}

pub struct OwnedView {
    repr: OwnedRepr,
}

impl<'a> View<'a> {
    pub fn from_slice(slice: &'a [u8]) -> Self {
        Self {
            repr: Repr::Borrowed(slice),
            range: 0..slice.len(),
        }
    }

    pub fn from_shared(shared: Shared) -> Self {
        let len = shared.len();
        Self {
            repr: Repr::Shared(shared),
            range: 0..len,
        }
    }

    pub fn from_pooled(pooled: Pooled) -> Self {
        let len = pooled.len();
        Self {
            repr: Repr::Pooled(pooled),
            range: 0..len,
        }
    }

    pub fn from_shared_range(shared: Shared, range: Range<usize>) -> Self {
        assert!(range.start <= range.end, "buffer view range is reversed");
        assert!(
            range.end <= shared.len(),
            "buffer view range is out of bounds"
        );
        Self {
            repr: Repr::Shared(shared),
            range,
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.range.len()
    }

    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.repr {
            Repr::Borrowed(slice) => &slice[self.range.clone()],
            Repr::Pooled(pooled) => &pooled.as_slice()[self.range.clone()],
            Repr::Shared(shared) => &shared[self.range.clone()],
        }
    }

    pub fn slice(mut self, range: Range<usize>) -> Self {
        assert!(range.start <= range.end, "buffer view range is reversed");
        let start = self
            .range
            .start
            .checked_add(range.start)
            .expect("buffer view range overflow");
        let end = self
            .range
            .start
            .checked_add(range.end)
            .expect("buffer view range overflow");
        assert!(end <= self.range.end, "buffer view range is out of bounds");
        self.range = start..end;
        self
    }

    pub fn into_shared(self) -> Shared {
        match self.repr {
            Repr::Borrowed(slice) => Shared::copy_from_slice(&slice[self.range]),
            Repr::Pooled(pooled) => Shared::copy_from_slice(&pooled.as_slice()[self.range]),
            Repr::Shared(shared) => shared.slice(self.range),
        }
    }

    #[inline]
    pub fn into_owned(self) -> OwnedView {
        match self.repr {
            Repr::Borrowed(slice) => {
                OwnedView::from_shared(Shared::copy_from_slice(&slice[self.range]))
            }
            Repr::Pooled(pooled) => OwnedView {
                repr: OwnedRepr::Pooled {
                    pooled,
                    start: self.range.start as u32,
                    len: self.range.len() as u32,
                },
            },
            Repr::Shared(mut shared) => {
                shared.advance(self.range.start);
                shared.truncate(self.range.len());
                OwnedView::from_shared(shared)
            }
        }
    }
}

impl OwnedView {
    fn from_shared(shared: Shared) -> Self {
        Self {
            repr: OwnedRepr::Shared(shared),
        }
    }

    #[inline]
    pub fn len(&self) -> usize {
        match &self.repr {
            OwnedRepr::Pooled { len, .. } => *len as usize,
            OwnedRepr::Shared(shared) => shared.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn slice(mut self, range: Range<usize>) -> Self {
        assert!(
            range.start <= range.end,
            "owned buffer view range is reversed"
        );
        assert!(
            range.end <= self.len(),
            "owned buffer view range is out of bounds"
        );
        match &mut self.repr {
            OwnedRepr::Pooled { start, len, .. } => {
                *start += range.start as u32;
                *len = range.len() as u32;
            }
            OwnedRepr::Shared(shared) => {
                shared.advance(range.start);
                shared.truncate(range.len());
            }
        }
        self
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        match &self.repr {
            OwnedRepr::Pooled { pooled, start, len } => {
                &pooled.as_slice()[*start as usize..(*start + *len) as usize]
            }
            OwnedRepr::Shared(shared) => shared.as_slice(),
        }
    }

    #[inline]
    pub fn advance(&mut self, n: usize) {
        assert!(
            n <= self.len(),
            "owned buffer view advance is out of bounds"
        );
        match &mut self.repr {
            OwnedRepr::Pooled { start, len, .. } => {
                *start += n as u32;
                *len -= n as u32;
            }
            OwnedRepr::Shared(shared) => shared.advance(n),
        }
    }
}

impl AsRef<[u8]> for OwnedView {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsRef<[u8]> for View<'_> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<'a> From<&'a [u8]> for View<'a> {
    fn from(value: &'a [u8]) -> Self {
        Self::from_slice(value)
    }
}

impl From<Shared> for View<'static> {
    fn from(value: Shared) -> Self {
        Self::from_shared(value)
    }
}

impl From<Pooled> for View<'static> {
    fn from(value: Pooled) -> Self {
        Self::from_pooled(value)
    }
}

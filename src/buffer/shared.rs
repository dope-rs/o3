use std::ops::{Bound, Deref, DerefMut, RangeBounds};

use super::raw::{Raw, RawMut};

#[derive(Clone)]
pub struct Shared {
    repr: SharedRepr,
}

#[derive(Clone)]
enum SharedRepr {
    Static(&'static [u8]),
    Owned { buf: Raw, start: u32, len: u32 },
}

impl Shared {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            repr: SharedRepr::Static(&[]),
        }
    }

    #[must_use]
    pub const fn from_static(s: &'static [u8]) -> Self {
        Self {
            repr: SharedRepr::Static(s),
        }
    }

    pub fn from_raw_range(buf: Raw, start: u32, len: u32) -> Self {
        assert!(
            start
                .checked_add(len)
                .is_some_and(|end| end <= buf.capacity()),
            "buffer::Shared::from_raw_range: range out of bounds (start={start}, len={len}, capacity={})",
            buf.capacity()
        );
        Self {
            repr: SharedRepr::Owned { buf, start, len },
        }
    }

    #[must_use]
    pub fn copy_from_slice(s: &[u8]) -> Self {
        if s.is_empty() {
            return Self::new();
        }
        let len = s.len();
        assert!(
            len <= u32::MAX as usize,
            "buffer::Shared: payload too large ({len}, max {})",
            u32::MAX
        );
        Self {
            repr: SharedRepr::Owned {
                buf: Raw::from_slice(s),
                start: 0,
                len: len as u32,
            },
        }
    }

    pub fn len(&self) -> usize {
        match &self.repr {
            SharedRepr::Static(s) => s.len(),
            SharedRepr::Owned { len, .. } => *len as usize,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        match &self.repr {
            SharedRepr::Static(s) => s,
            SharedRepr::Owned { buf, start, len } => unsafe {
                std::slice::from_raw_parts(buf.data_ptr().add(*start as usize), *len as usize)
            },
        }
    }

    #[must_use]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Self {
        let len = self.len();
        let start = match range.start_bound() {
            Bound::Included(&n) => n,
            Bound::Excluded(&n) => n + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&n) => n + 1,
            Bound::Excluded(&n) => n,
            Bound::Unbounded => len,
        };
        assert!(
            start <= end && end <= len,
            "buffer::Shared::slice: range out of bounds"
        );
        if start == end {
            return Self::new();
        }
        match &self.repr {
            SharedRepr::Static(s) => Self::from_static(&s[start..end]),
            SharedRepr::Owned {
                buf, start: cur, ..
            } => Self {
                repr: SharedRepr::Owned {
                    buf: buf.clone(),
                    start: *cur + start as u32,
                    len: (end - start) as u32,
                },
            },
        }
    }

    pub fn advance(&mut self, n: usize) {
        let len = self.len();
        assert!(n <= len, "buffer::Shared::advance: out of bounds");
        match &mut self.repr {
            SharedRepr::Static(s) => *s = &s[n..],
            SharedRepr::Owned { start, len, .. } => {
                *start += n as u32;
                *len -= n as u32;
            }
        }
    }

    #[must_use]
    pub fn split_to(&mut self, at: usize) -> Self {
        let head = self.slice(..at);
        self.advance(at);
        head
    }

    pub fn clear(&mut self) {
        self.repr = SharedRepr::Static(&[]);
    }

    #[must_use]
    pub fn freeze(self) -> Self {
        self
    }

    pub fn truncate(&mut self, n: usize) {
        let len = self.len();
        if n >= len {
            return;
        }
        match &mut self.repr {
            SharedRepr::Static(s) => *s = &s[..n],
            SharedRepr::Owned { len, .. } => *len = n as u32,
        }
    }
}

impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}

impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Deref for Shared {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl From<&'static [u8]> for Shared {
    fn from(value: &'static [u8]) -> Self {
        Self::from_static(value)
    }
}

impl<const N: usize> From<&'static [u8; N]> for Shared {
    fn from(value: &'static [u8; N]) -> Self {
        Self::from_static(value)
    }
}

impl From<Vec<u8>> for Shared {
    fn from(value: Vec<u8>) -> Self {
        Self::copy_from_slice(&value)
    }
}

impl From<Owned> for Shared {
    fn from(value: Owned) -> Self {
        value.freeze()
    }
}

impl From<String> for Shared {
    fn from(value: String) -> Self {
        Self::copy_from_slice(value.as_bytes())
    }
}

impl From<&str> for Shared {
    fn from(value: &str) -> Self {
        Self::copy_from_slice(value.as_bytes())
    }
}

impl PartialEq for Shared {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl PartialEq<[u8]> for Shared {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

impl PartialEq<&[u8]> for Shared {
    fn eq(&self, other: &&[u8]) -> bool {
        self.as_slice() == *other
    }
}

impl Eq for Shared {}

impl std::hash::Hash for Shared {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl std::fmt::Debug for Shared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Shared").field("len", &self.len()).finish()
    }
}

pub struct Owned {
    raw: Option<RawMut>,
    len: u32,
}

impl Owned {
    #[must_use]
    pub const fn new() -> Self {
        Self { raw: None, len: 0 }
    }

    #[must_use]
    pub fn with_capacity(cap: usize) -> Self {
        if cap == 0 {
            return Self::new();
        }
        Self {
            raw: Some(RawMut::with_capacity(cap)),
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn capacity(&self) -> usize {
        self.raw.as_ref().map_or(0, |r| r.capacity() as usize)
    }

    pub fn as_slice(&self) -> &[u8] {
        match self.raw.as_ref() {
            Some(raw) => unsafe { std::slice::from_raw_parts(raw.data_ptr(), self.len as usize) },
            None => &[],
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.len as usize;
        match self.raw.as_mut() {
            Some(raw) => unsafe { std::slice::from_raw_parts_mut(raw.data_mut_ptr(), len) },
            None => &mut [],
        }
    }

    pub fn extend_from_slice(&mut self, src: &[u8]) {
        if src.is_empty() {
            return;
        }
        let new_len = self.len as usize + src.len();
        self.reserve_total(new_len);
        let raw = self.raw.as_mut().expect("reserve_total ensured raw exists");
        unsafe {
            std::ptr::copy_nonoverlapping(
                src.as_ptr(),
                raw.data_mut_ptr().add(self.len as usize),
                src.len(),
            );
        }
        self.len = new_len as u32;
    }

    pub fn reserve(&mut self, additional: usize) {
        let target = (self.len as usize)
            .checked_add(additional)
            .expect("overflow");
        self.reserve_total(target);
    }

    fn reserve_total(&mut self, target: usize) {
        let cur = self.capacity();
        if target <= cur {
            return;
        }
        let new_cap = std::cmp::max(target, std::cmp::max(cur * 2, 8));
        let mut new_raw = RawMut::with_capacity(new_cap);
        if self.len > 0
            && let Some(old) = self.raw.as_ref()
        {
            unsafe {
                std::ptr::copy_nonoverlapping(
                    old.data_ptr(),
                    new_raw.data_mut_ptr(),
                    self.len as usize,
                );
            }
        }
        self.raw = Some(new_raw);
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn truncate(&mut self, len: usize) {
        if (len as u64) < self.len as u64 {
            self.len = len as u32;
        }
    }

    pub fn push(&mut self, byte: u8) {
        self.extend_from_slice(&[byte]);
    }

    pub unsafe fn set_len(&mut self, len: usize) {
        debug_assert!(len <= self.capacity());
        self.len = len as u32;
    }

    pub fn spare_capacity_mut(&mut self) -> &mut [std::mem::MaybeUninit<u8>] {
        let len = self.len as usize;
        match self.raw.as_mut() {
            Some(raw) => {
                let cap = raw.capacity() as usize;
                unsafe {
                    let ptr = raw.data_mut_ptr().add(len) as *mut std::mem::MaybeUninit<u8>;
                    std::slice::from_raw_parts_mut(ptr, cap - len)
                }
            }
            None => &mut [],
        }
    }

    #[must_use]
    pub fn split(&mut self) -> Shared {
        let len = self.len;
        self.len = 0;
        match self.raw.take() {
            Some(raw_mut) => Shared::from_raw_range(raw_mut.freeze(), 0, len),
            None => Shared::new(),
        }
    }

    #[must_use]
    pub fn split_to(&mut self, at: usize) -> Owned {
        assert!(at <= self.len as usize, "Owned::split_to: out of bounds");
        if at == 0 {
            return Owned::new();
        }
        let mut head = Owned::with_capacity(at);
        let raw = self.raw.as_mut().expect("len > 0 implies raw exists");
        unsafe {
            std::ptr::copy_nonoverlapping(
                raw.data_ptr(),
                head.raw.as_mut().unwrap().data_mut_ptr(),
                at,
            );
            head.len = at as u32;
            let remaining = self.len as usize - at;
            if remaining > 0 {
                std::ptr::copy(raw.data_ptr().add(at), raw.data_mut_ptr(), remaining);
            }
            self.len = remaining as u32;
        }
        head
    }

    #[must_use]
    pub fn split_off(&mut self, at: usize) -> Owned {
        assert!(at <= self.len as usize, "Owned::split_off: out of bounds");
        let tail_len = self.len as usize - at;
        if tail_len == 0 {
            return Owned::new();
        }
        let mut tail = Owned::with_capacity(tail_len);
        let raw = self.raw.as_ref().expect("len > 0 implies raw exists");
        unsafe {
            std::ptr::copy_nonoverlapping(
                raw.data_ptr().add(at),
                tail.raw.as_mut().unwrap().data_mut_ptr(),
                tail_len,
            );
            tail.len = tail_len as u32;
        }
        self.len = at as u32;
        tail
    }

    #[must_use]
    pub fn freeze(self) -> Shared {
        let Self { raw, len } = self;
        match raw {
            Some(raw_mut) => Shared::from_raw_range(raw_mut.freeze(), 0, len),
            None => Shared::new(),
        }
    }
}

impl Default for Owned {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for Owned {
    fn clone(&self) -> Self {
        let mut new = Self::with_capacity(self.len as usize);
        new.extend_from_slice(self.as_slice());
        new
    }
}

impl AsRef<[u8]> for Owned {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl AsMut<[u8]> for Owned {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl Deref for Owned {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl DerefMut for Owned {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl From<Vec<u8>> for Owned {
    fn from(value: Vec<u8>) -> Self {
        let mut o = Self::with_capacity(value.len());
        o.extend_from_slice(&value);
        o
    }
}

impl From<&[u8]> for Owned {
    fn from(value: &[u8]) -> Self {
        let mut o = Self::with_capacity(value.len());
        o.extend_from_slice(value);
        o
    }
}

impl PartialEq for Owned {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Owned {}

impl std::hash::Hash for Owned {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl std::fmt::Debug for Owned {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Owned").field("len", &self.len()).finish()
    }
}

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;
use std::mem::{ManuallyDrop, replace, take};
use std::ops::{Deref, DerefMut};
use std::ptr::{NonNull, copy_nonoverlapping};
use std::slice::{from_raw_parts, from_raw_parts_mut};

use super::SpareWriter;
use super::raw::RawMut;
use super::shared::Shared;

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum Repr {
    Native,
    Vec,
}

pub struct Owned {
    ptr: NonNull<u8>,
    capacity: usize,
    len: u32,
    repr: Repr,
    marker: PhantomData<*mut ()>,
}

pub struct Writer<'a> {
    owned: &'a mut Owned,
    ptr: NonNull<u8>,
    capacity: usize,
    len: usize,
    start: usize,
}

impl Writer<'_> {
    pub fn len(&self) -> usize {
        self.len - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.len == self.start
    }

    pub fn extend_from_slice(&mut self, src: &[u8]) {
        let end = self
            .len
            .checked_add(src.len())
            .filter(|&len| u32::try_from(len).is_ok())
            .expect("buffer capacity overflow");
        if end > self.capacity {
            self.grow(src.len());
        }
        unsafe { copy_nonoverlapping(src.as_ptr(), self.ptr.as_ptr().add(self.len), src.len()) };
        self.len = end;
    }

    pub fn push(&mut self, byte: u8) {
        self.extend_from_slice(&[byte]);
    }

    pub fn finish(self) -> usize {
        self.len - self.start
    }

    #[cold]
    fn grow(&mut self, additional: usize) {
        self.commit();
        self.owned.reserve(additional);
        self.ptr = self.owned.ptr;
        self.capacity = self.owned.capacity;
    }

    fn commit(&mut self) {
        self.owned.len = self.len as u32;
    }
}

impl Drop for Writer<'_> {
    fn drop(&mut self) {
        self.commit();
    }
}

impl Owned {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            ptr: NonNull::dangling(),
            capacity: 0,
            len: 0,
            repr: Repr::Native,
            marker: PhantomData,
        }
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        if capacity == 0 {
            return Self::new();
        }
        let raw = RawMut::with_capacity(capacity);
        Self {
            ptr: raw.into_data(),
            capacity,
            len: 0,
            repr: Repr::Native,
            marker: PhantomData,
        }
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn as_slice(&self) -> &[u8] {
        unsafe { from_raw_parts(self.ptr.as_ptr(), self.len()) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { from_raw_parts_mut(self.ptr.as_ptr(), self.len()) }
    }

    pub fn extend_from_slice(&mut self, src: &[u8]) {
        if src.is_empty() {
            return;
        }
        let start = self.len();
        let len = start
            .checked_add(src.len())
            .filter(|&len| u32::try_from(len).is_ok())
            .expect("buffer capacity overflow");
        self.reserve_total(len);
        unsafe { copy_nonoverlapping(src.as_ptr(), self.ptr.as_ptr().add(start), src.len()) };
        self.len = len as u32;
    }

    pub fn reserve(&mut self, additional: usize) {
        let target = self
            .len()
            .checked_add(additional)
            .filter(|&len| u32::try_from(len).is_ok())
            .expect("buffer capacity overflow");
        self.reserve_total(target);
    }

    fn reserve_total(&mut self, target: usize) {
        if target <= self.capacity {
            return;
        }
        if self.repr == Repr::Vec {
            let len = self.len();
            self.vec().reserve(target - len);
            return;
        }
        let capacity = self
            .capacity
            .saturating_mul(2)
            .min(u32::MAX as usize)
            .max(target)
            .max(8);
        let mut raw = RawMut::with_capacity(capacity);
        if self.len != 0 {
            unsafe { copy_nonoverlapping(self.ptr.as_ptr(), raw.data_mut_ptr(), self.len()) };
        }
        let ptr = raw.into_data();
        if self.capacity != 0 {
            unsafe { drop(RawMut::from_data(self.ptr)) };
        }
        self.ptr = ptr;
        self.capacity = capacity;
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn truncate(&mut self, len: usize) {
        if len < self.len() {
            self.len = len as u32;
        }
    }

    #[must_use]
    pub fn copy_from_slice(bytes: &[u8]) -> Self {
        Self::from(bytes)
    }

    pub fn push(&mut self, byte: u8) {
        self.extend_from_slice(&[byte]);
    }

    pub fn writer(&mut self, additional: usize) -> Writer<'_> {
        self.reserve(additional);
        let len = self.len();
        let ptr = self.ptr;
        let capacity = self.capacity;
        Writer {
            owned: self,
            ptr,
            capacity,
            len,
            start: len,
        }
    }

    pub fn spare_writer(&mut self) -> SpareWriter<'_> {
        let capacity = self.capacity.min(u32::MAX as usize);
        let ptr = unsafe { self.ptr.as_ptr().add(self.len()).cast() };
        unsafe { SpareWriter::new(ptr, capacity - self.len(), &mut self.len) }
    }

    #[must_use]
    pub fn split(&mut self) -> Shared {
        take(self).freeze()
    }

    #[must_use]
    pub fn split_to(&mut self, at: usize) -> Self {
        assert!(at <= self.len(), "Owned::split_to: out of bounds");
        if self.repr == Repr::Vec {
            let mut buf = self.vec();
            let tail = buf.split_off(at);
            return Self::from(replace(&mut *buf, tail));
        }
        let remaining = self.len() - at;
        let mut tail = Self::with_capacity(remaining);
        if remaining != 0 {
            unsafe { copy_nonoverlapping(self.ptr.as_ptr().add(at), tail.ptr.as_ptr(), remaining) };
            tail.len = remaining as u32;
        }
        let mut head = replace(self, tail);
        head.len = at as u32;
        head
    }

    #[must_use]
    pub fn split_off(&mut self, at: usize) -> Self {
        assert!(at <= self.len(), "Owned::split_off: out of bounds");
        if self.repr == Repr::Vec {
            return Self::from(self.vec().split_off(at));
        }
        let len = self.len() - at;
        let mut tail = Self::with_capacity(len);
        if len != 0 {
            unsafe { copy_nonoverlapping(self.ptr.as_ptr().add(at), tail.ptr.as_ptr(), len) };
            tail.len = len as u32;
        }
        self.len = at as u32;
        tail
    }

    #[must_use]
    pub fn freeze(self) -> Shared {
        if self.is_empty() {
            return Shared::new();
        }
        let this = ManuallyDrop::new(self);
        match this.repr {
            Repr::Native => {
                let raw = unsafe { RawMut::from_data(this.ptr) }.freeze();
                Shared::from_raw_range(raw, 0, this.len)
            }
            Repr::Vec => {
                let buf =
                    unsafe { Vec::from_raw_parts(this.ptr.as_ptr(), this.len(), this.capacity) };
                Shared::from_vec(buf)
            }
        }
    }

    fn vec(&mut self) -> VecGuard<'_> {
        debug_assert!(self.repr == Repr::Vec);
        let buf = unsafe { Vec::from_raw_parts(self.ptr.as_ptr(), self.len(), self.capacity) };
        self.ptr = NonNull::dangling();
        self.capacity = 0;
        self.len = 0;
        self.repr = Repr::Native;
        VecGuard {
            owner: self,
            buf: ManuallyDrop::new(buf),
        }
    }
}

struct VecGuard<'a> {
    owner: &'a mut Owned,
    buf: ManuallyDrop<Vec<u8>>,
}

impl Deref for VecGuard<'_> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl DerefMut for VecGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}

impl Drop for VecGuard<'_> {
    fn drop(&mut self) {
        self.owner.ptr = unsafe { NonNull::new_unchecked(self.buf.as_mut_ptr()) };
        self.owner.capacity = self.buf.capacity();
        self.owner.len = self.buf.len() as u32;
        self.owner.repr = Repr::Vec;
    }
}

impl Drop for Owned {
    fn drop(&mut self) {
        match self.repr {
            Repr::Native if self.capacity != 0 => unsafe {
                drop(RawMut::from_data(self.ptr));
            },
            Repr::Vec => unsafe {
                drop(Vec::from_raw_parts(
                    self.ptr.as_ptr(),
                    self.len(),
                    self.capacity,
                ));
            },
            Repr::Native => {}
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
        Self::copy_from_slice(self.as_slice())
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

    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl DerefMut for Owned {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl From<&[u8]> for Owned {
    fn from(value: &[u8]) -> Self {
        let mut buf = Self::with_capacity(value.len());
        buf.extend_from_slice(value);
        buf
    }
}

impl From<Vec<u8>> for Owned {
    fn from(buf: Vec<u8>) -> Self {
        let len = u32::try_from(buf.len()).expect("buffer capacity overflow");
        let mut buf = ManuallyDrop::new(buf);
        Self {
            ptr: unsafe { NonNull::new_unchecked(buf.as_mut_ptr()) },
            capacity: buf.capacity(),
            len,
            repr: Repr::Vec,
            marker: PhantomData,
        }
    }
}

impl PartialEq for Owned {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for Owned {}

impl Hash for Owned {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state);
    }
}

impl fmt::Debug for Owned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Owned").field("len", &self.len()).finish()
    }
}

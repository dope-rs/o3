use std::{convert, error, fmt, mem, ptr, slice};

use crate::buffer::{self, queue};

fn append_vec_slices<const N: usize>(out: &mut Vec<u8>, slices: [&[u8]; N]) {
    let additional = slices
        .iter()
        .fold(0usize, |len, slice| len.saturating_add(slice.len()));
    let start = out.len();
    out.reserve(additional);
    let mut offset = start;
    for slice in slices {
        // SAFETY: the aggregate reserve covers every copy and safe borrowing
        // prevents the sources from aliasing this vector.
        unsafe {
            ptr::copy_nonoverlapping(slice.as_ptr(), out.as_mut_ptr().add(offset), slice.len())
        };
        offset += slice.len();
    }
    // SAFETY: every byte in `start..offset` was initialized above.
    unsafe { out.set_len(offset) };
}

pub struct SpareWriter<'a> {
    ptr: ptr::NonNull<mem::MaybeUninit<u8>>,
    capacity: usize,
    written: usize,
    target: &'a mut u32,
    _thread: crate::ThreadBound,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExactError {
    expected: usize,
    actual: usize,
}

impl fmt::Display for ExactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "exact write incomplete: expected {}, wrote {}",
            self.expected, self.actual
        )
    }
}

impl error::Error for ExactError {}

/// An exact-length write reservation that rolls back unless committed.
/// Safe writes are confined to the reserved extent.
#[must_use = "dropping a write transaction rolls its bytes back"]
pub struct Txn<'writer, 'target> {
    writer: &'writer mut SpareWriter<'target>,
    start: usize,
    end: usize,
    committed: bool,
}

impl<'a> SpareWriter<'a> {
    pub fn len(&self) -> usize {
        self.written
    }

    pub fn is_empty(&self) -> bool {
        self.written == 0
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        unsafe { slice::from_raw_parts_mut(self.ptr.as_ptr().cast(), self.written) }
    }

    pub fn truncate(&mut self, len: usize) {
        self.written = self.written.min(len);
    }

    /// Reserves exactly `len` bytes for an all-or-nothing safe write.
    pub fn try_transaction(&mut self, len: usize) -> Result<Txn<'_, 'a>, buffer::CapacityError> {
        let end = self
            .written
            .checked_add(len)
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, self.capacity))?;
        if end > self.capacity {
            return Err(buffer::CapacityError::new(end, self.capacity));
        }
        Ok(Txn {
            start: self.written,
            end,
            writer: self,
            committed: false,
        })
    }

    pub fn try_push(&mut self, byte: u8) -> Result<(), buffer::CapacityError> {
        if self.written == self.capacity {
            return Err(buffer::CapacityError::new(self.written + 1, self.capacity));
        }
        unsafe {
            self.ptr
                .as_ptr()
                .add(self.written)
                .write(mem::MaybeUninit::new(byte))
        };
        self.written += 1;
        Ok(())
    }

    /// Appends one contiguous slice after validating its complete length.
    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        let end = self
            .written
            .checked_add(src.len())
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, self.capacity))?;
        if end > self.capacity {
            return Err(buffer::CapacityError::new(end, self.capacity));
        }
        unsafe {
            ptr::copy_nonoverlapping(
                src.as_ptr(),
                self.ptr.as_ptr().add(self.written).cast(),
                src.len(),
            )
        };
        self.written = end;
        Ok(())
    }

    /// Appends every slice after validating their aggregate length.
    ///
    /// On failure, neither the writer length nor its target length changes.
    pub fn try_extend_from_slices<const N: usize>(
        &mut self,
        slices: [&[u8]; N],
    ) -> Result<(), buffer::CapacityError> {
        let end = buffer::checked_append_len(self.written, self.capacity, &slices)?;
        let mut offset = self.written;
        for src in slices {
            unsafe {
                ptr::copy_nonoverlapping(
                    src.as_ptr(),
                    self.ptr.as_ptr().add(offset).cast(),
                    src.len(),
                )
            };
            offset += src.len();
        }
        self.written = end;
        Ok(())
    }

    pub fn finish(self) -> usize {
        self.written
    }

    pub(in crate::buffer) unsafe fn new(
        ptr: *mut mem::MaybeUninit<u8>,
        capacity: usize,
        target: &'a mut u32,
    ) -> Self {
        use crate::ThreadBound;
        debug_assert!(capacity <= (u32::MAX - *target) as usize);
        Self {
            ptr: unsafe { ptr::NonNull::new_unchecked(ptr) },
            capacity,
            written: 0,
            target,
            _thread: ThreadBound::NEW,
        }
    }

    fn commit(&mut self) {
        if self.is_empty() {
            return;
        }
        *self.target = self.target.wrapping_add(self.written as u32);
        self.written = 0;
    }
}

impl Txn<'_, '_> {
    fn written(&self) -> usize {
        self.writer.written - self.start
    }

    pub fn remaining(&self) -> usize {
        self.end - self.writer.written
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), buffer::CapacityError> {
        if src.len() > self.remaining() {
            return Err(buffer::CapacityError::new(
                self.written().saturating_add(src.len()),
                self.end - self.start,
            ));
        }
        self.writer.try_extend(src)
    }

    /// Returns the initialized portion of this transaction for in-place work.
    pub fn initialized_mut(&mut self) -> &mut [u8] {
        &mut self.writer.as_mut_slice()[self.start..]
    }

    /// Makes the exact initialized reservation visible in the parent writer.
    pub fn commit(mut self) -> Result<(), ExactError> {
        let actual = self.written();
        let expected = self.end - self.start;
        if actual != expected {
            return Err(ExactError { expected, actual });
        }
        self.committed = true;
        Ok(())
    }
}

impl Drop for Txn<'_, '_> {
    fn drop(&mut self) {
        if !self.committed {
            self.writer.truncate(self.start);
        }
    }
}

impl Drop for SpareWriter<'_> {
    fn drop(&mut self) {
        self.commit();
    }
}

/// A byte destination whose individual writes either append completely or
/// leave its logical output unchanged.
pub trait ByteSink {
    type Error;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error>;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error>;
}

impl ByteSink for Vec<u8> {
    type Error = convert::Infallible;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.push(byte);
        Ok(())
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.extend_from_slice(bytes);
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        append_vec_slices(self, slices);
        Ok(())
    }
}

impl ByteSink for SpareWriter<'_> {
    type Error = buffer::CapacityError;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.try_push(byte)
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.try_extend(bytes)
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_extend_from_slices(slices)
    }
}

impl ByteSink for queue::Ring {
    type Error = buffer::CapacityError;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        self.try_push(byte)
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        self.try_extend(bytes)
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        self.try_extend_from_slices(slices)
    }
}

/// A checked cursor over an initialized output slice.
pub struct SliceWriter<'a> {
    out: &'a mut [u8],
    written: usize,
}

impl<'a> SliceWriter<'a> {
    pub fn new(out: &'a mut [u8]) -> Self {
        Self { out, written: 0 }
    }

    pub fn finish(self) -> usize {
        self.written
    }
}

impl ByteSink for SliceWriter<'_> {
    type Error = buffer::CapacityError;

    fn write_byte(&mut self, byte: u8) -> Result<(), Self::Error> {
        if self.written == self.out.len() {
            return Err(buffer::CapacityError::new(
                self.written.saturating_add(1),
                self.out.len(),
            ));
        }
        self.out[self.written] = byte;
        self.written += 1;
        Ok(())
    }

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let end = self
            .written
            .checked_add(bytes.len())
            .ok_or_else(|| buffer::CapacityError::new(usize::MAX, self.out.len()))?;
        if end > self.out.len() {
            return Err(buffer::CapacityError::new(end, self.out.len()));
        }
        self.out[self.written..end].copy_from_slice(bytes);
        self.written = end;
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        let end = buffer::checked_append_len(self.written, self.out.len(), &slices)?;
        let mut offset = self.written;
        for src in slices {
            let next = offset + src.len();
            self.out[offset..next].copy_from_slice(src);
            offset = next;
        }
        self.written = end;
        Ok(())
    }
}

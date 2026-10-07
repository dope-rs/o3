use std::mem::MaybeUninit;

use write::{Reserved, consume};

use crate::buffer::{self, write};

pub(in crate::buffer::write) mod staging;

/// # Safety
/// Exclusively retain `len` initialized bytes beyond the unchanged vector length, within capacity.
pub(in crate::buffer::write) unsafe fn publish_suffix(out: &mut Vec<u8>, len: usize) -> &mut [u8] {
    let start = out.len();
    // SAFETY: the caller retained this initialized suffix within capacity.
    unsafe { out.set_len(start + len) };
    // SAFETY: publication cannot reallocate; this initialized suffix retains out's
    // exclusive borrow. An empty suffix uses the valid end or dangling empty pointer.
    unsafe { buffer::raw::range_mut(out.as_mut_ptr(), start, len) }
}

/// Borrows a vector's reserved suffix without publishing bytes until `commit`.
#[derive(Debug)]
#[must_use = "call commit to publish initialized bytes"]
pub struct Staged<'a, L = u32> {
    pub(in crate::buffer::write) out: &'a mut Vec<u8>,
    pub(in crate::buffer::write) len: L,
    pub(in crate::buffer::write) capacity: L,
}

impl<'a, L: buffer::raw::Length> Staged<'a, L> {
    /// Grows exactly as needed and retains the admitted unpublished suffix.
    pub fn try_grow(out: &'a mut Vec<u8>, limit: L) -> Result<Self, crate::alloc::Error> {
        Ok(Reserved::try_grow(out, limit)?.into_staged())
    }

    /// Reserves and retains an exact unpublished suffix in one admission.
    pub fn try_reserve(out: &'a mut Vec<u8>, limit: L) -> Result<Self, crate::alloc::Error> {
        Ok(Reserved::try_reserve(out, limit)?.into_staged())
    }

    /// Admits an exact suffix limit without allocating or changing the vector.
    pub fn try_new(out: &'a mut Vec<u8>, limit: L) -> Result<Self, consume::Error> {
        Ok(Reserved::try_new(out, limit)?.into_staged())
    }

    pub fn len(&self) -> usize {
        self.len.into_usize()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn capacity(&self) -> usize {
        self.capacity.into_usize()
    }

    /// Admits a fully initialized suffix without publishing it.
    pub fn try_complete(self) -> Result<Self, consume::Error> {
        consume::Error::check(self.capacity(), self.len())?;
        Ok(self)
    }

    /// Publishes and returns only the initialized staged prefix under the original borrow.
    pub fn commit(self) -> &'a mut [u8] {
        let len = self.len();
        // SAFETY: admission bounds the suffix by this retained vector's capacity;
        // consuming the stage transfers its exclusive initialized prefix for 'a.
        unsafe { publish_suffix(self.out, len) }
    }
}

impl<L: buffer::raw::Length> write::Sink for Staged<'_, L> {
    type Error = consume::Error;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let added = bytes.len();
        consume::Error::check(added, self.capacity() - self.len())?;
        buffer::raw::Copy::copy(Init {
            stage: self,
            sources: bytes,
            added,
        });
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        let added = consume::Error::checked_end(0, self.capacity() - self.len(), &slices)?;
        buffer::raw::Copy::copy(Init {
            stage: self,
            sources: slices,
            added,
        });
        Ok(())
    }
}

struct Init<'stage, 'out, L, S> {
    stage: &'stage mut Staged<'out, L>,
    sources: S,
    added: usize,
}

// SAFETY: complete admission retains this exact stage and disjoint sources;
// added publishes precisely their initialized suffix without changing the vector.
unsafe impl<L: buffer::raw::Length, S: buffer::raw::SliceExt> buffer::raw::Copy
    for Init<'_, '_, L, S>
{
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        let start = self.stage.out.len() + self.stage.len();
        (&self.sources, self.stage.out.as_mut_ptr(), start)
    }

    unsafe fn publish(self) {
        // SAFETY: admission bounds the combined initialized prefix by this stage's L capacity.
        self.stage.len = unsafe { self.stage.len.add(self.added) };
    }
}

const _: () = {
    assert!(size_of::<Staged<'static>>() == size_of::<usize>() + 2 * size_of::<u32>());
    assert!(size_of::<Option<Staged<'static>>>() == size_of::<Staged<'static>>());
    assert!(!std::mem::needs_drop::<Staged<'static>>());
    assert!(size_of::<Staged<'static, usize>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Staged<'static, usize>>>() == size_of::<Staged<'static, usize>>());
    assert!(!std::mem::needs_drop::<Staged<'static, usize>>());
};

// SAFETY: the vector's unchanged length selects the stage's exclusive suffix;
// its staged count advances only after these exact bytes are initialized.
unsafe impl buffer::raw::Prefix for Staged<'_> {
    fn prefix(&self) -> (*mut MaybeUninit<u8>, u32) {
        // SAFETY: the vector's original initialized length is within its allocation.
        let pointer = unsafe { self.out.as_ptr().add(self.out.len()) };
        (pointer.cast_mut().cast(), self.len)
    }
}

// SAFETY: the retained vector cannot grow or publish during this stage. Its
// admitted suffix is exclusive, isize-bounded and disjoint from the staged count.
unsafe impl buffer::raw::Exclusive for Staged<'_> {
    fn as_mut_ptr(&mut self) -> *mut MaybeUninit<u8> {
        let start = self.out.len();
        // SAFETY: the unchanged original length selects the admitted suffix.
        let pointer = unsafe { self.out.as_mut_ptr().add(start) };
        pointer.cast()
    }

    fn len_mut(&mut self) -> &mut u32 {
        &mut self.len
    }

    fn capacity(&self) -> u32 {
        self.capacity
    }
}

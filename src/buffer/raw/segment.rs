use std::ops;

use crate::{
    buffer,
    buffer::{resident, segments, segments::Seal, storage, storage::regions, write::consume},
    mem::rc,
};
/// Retained bytes that narrow without copying their payload.
/// # Safety
/// Shared access preserves the range, and consuming advances its exact prefix.
pub unsafe trait Segment: Seal + AsRef<[u8]> + consume::Consume + buffer::Residency {
    /// # Safety
    /// Preserve this owner's exact initialized extent and its original backing.
    unsafe fn extent(&mut self) -> &mut regions::Extent;

    fn try_clone(&self) -> Result<Self, rc::Error> {
        buffer::View::new(self).try_retain()
    }

    fn try_get(&self, range: ops::Range<usize>) -> Result<Option<Self>, rc::Error> {
        buffer::View::new(self)
            .get(range)
            .map(buffer::View::<Self>::try_retain)
            .transpose()
    }

    /// Retains an in-bounds owner subrange without copying its payload.
    /// # Safety
    /// `range` must lie within the owner's visible bytes.
    unsafe fn try_project(&self, range: ops::Range<usize>) -> Result<Self, rc::Error> {
        let bytes = unsafe { self.as_ref().get_unchecked(range) };
        buffer::View {
            source: self,
            bytes,
        }
        .try_retain()
    }

    /// Retains at most `limit` leading bytes without copying the payload.
    fn prefix(self, limit: usize) -> Self {
        Seal::truncate(self, limit)
    }

    /// Splits off at most `limit` visible bytes without copying the payload.
    fn try_split(mut self, limit: usize) -> Result<(Self, Option<Self>), (rc::Error, Self)> {
        let Some(tail) = segments::Tail::new(&mut self, limit) else {
            return Ok((self, None));
        };
        let prefix = match tail.try_commit() {
            Ok(result) => result,
            Err(error) => return Err((error, self)),
        };
        Ok((prefix, Some(self)))
    }

    /// Removes at most `amount` visible bytes without copying the payload.
    fn suffix(mut self, amount: usize) -> Option<Self> {
        segments::Tail::new(&mut self, amount)?.commit();
        Some(self)
    }
}
// SAFETY: shared storage changes its visible range only through exclusive access.
unsafe impl Segment for storage::Shared {
    unsafe fn extent(&mut self) -> &mut regions::Extent {
        // SAFETY: the caller preserves this exact owner and initialized subrange.
        unsafe { self.region.extent_mut() }
    }
}

// SAFETY: resident storage retains its range and domain until exclusive mutation.
unsafe impl Segment for resident::Shared<'_> {
    unsafe fn extent(&mut self) -> &mut regions::Extent {
        // SAFETY: the caller preserves this exact owner and initialized subrange.
        unsafe { self.region.extent_mut() }
    }
}

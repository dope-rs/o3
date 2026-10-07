use std::mem;

use crate::{
    buffer,
    buffer::{
        bytes::scope::Range,
        storage,
        storage::regions,
        write::{consume, consume::scope},
    },
};
/// A validated prefix tied to the exclusive owner and the input operation.
pub struct Consumed<'a, 'id, R: ?Sized, const MAX: u32> {
    source: &'a mut R,
    end: scope::End<'id, MAX>,
}

impl<'a, 'id, R: ?Sized, const MAX: u32> Consumed<'a, 'id, R, MAX> {
    /// Consumes this admitted prefix through an exact storage adapter.
    pub fn consume<S: buffer::raw::Consume + ?Sized>(self)
    where
        R: buffer::raw::Project<S>,
    {
        S::consume(Consumed {
            // SAFETY: consumption only restricts the adapter's exact inner extent.
            source: unsafe { self.source.as_inner_mut() },
            end: self.end,
        });
    }

    pub(in crate::buffer) fn new(source: &'a mut R, end: scope::End<'id, MAX>) -> Self {
        Self { source, end }
    }

    /// Refines a prefix's owner after matching a storage variant.
    /// # Safety
    /// `source` must expose the original `into_parts` owner's unchanged range.
    pub unsafe fn from_parts(source: &'a mut R, end: scope::End<'id, MAX>) -> Self {
        Self { source, end }
    }

    /// Projects the same initialized byte range through a storage adapter.
    /// # Safety
    /// The projected owner must expose exactly this owner's current bytes.
    pub unsafe fn map<S: ?Sized>(
        self,
        project: impl FnOnce(&'a mut R) -> &'a mut S,
    ) -> Consumed<'a, 'id, S, MAX> {
        Consumed {
            source: project(self.source),
            end: self.end,
        }
    }

    /// Transfers the exclusive owner and its bound to the final storage leaf.
    pub fn into_parts(self) -> (&'a mut R, scope::End<'id, MAX>) {
        (self.source, self.end)
    }
}

/// One selected prefix retaining its exclusive source through commitment.
pub(in crate::buffer) struct Selection<'a, S> {
    source: &'a mut S,
    len: usize,
}

impl<'a, S> Selection<'a, S> {
    pub(in crate::buffer) const fn len(&self) -> usize {
        self.len
    }

    /// Transfers the unchanged exclusive source and its admitted prefix length.
    pub(in crate::buffer) fn into_parts(self) -> (&'a mut S, usize) {
        (self.source, self.len)
    }
}

impl<'a, S, const MAX: u32> From<buffer::raw::Consumed<'a, '_, S, MAX>> for Selection<'a, S> {
    fn from(prefix: buffer::raw::Consumed<'a, '_, S, MAX>) -> Self {
        let (source, end) = prefix.into_parts();
        Self {
            source,
            len: end.count().get() as usize,
        }
    }
}

impl<'a, S: buffer::raw::Source> Selection<'a, S> {
    /// Admits one prefix length and retains its exact exclusive source.
    pub(in crate::buffer) fn try_new(
        source: &'a mut S,
        len: usize,
    ) -> Result<Self, consume::Error> {
        consume::Error::check(len, source.as_ref().len())?;
        Ok(Self { source, len })
    }

    pub(in crate::buffer) fn new(source: &'a mut S, max: usize) -> Self {
        let len = max.min(source.as_ref().len());
        Self { source, len }
    }
}

impl<S: AsRef<[u8]>> Selection<'_, S> {
    /// The exact bytes this admitted prefix still exposes.
    pub(in crate::buffer) fn bytes(&self) -> &[u8] {
        self.source.as_ref()
    }
}

impl<'d> Selection<'_, &'d mut [u8]> {
    pub(in crate::buffer) fn take(self) -> &'d mut [u8] {
        let Self { source, len } = self;
        let bytes = mem::take(source);
        // SAFETY: this prefix retains the exact admitted slice through exclusive mutation.
        let (head, tail) = unsafe { bytes.split_at_mut_unchecked(len) };
        *source = tail;
        head
    }
}

impl<S: crate::buffer::raw::Store> Selection<'_, S> {
    pub(in crate::buffer) fn consume(self) {
        S::project(Range::select::<true, _>(self));
    }

    pub(in crate::buffer) fn commit(self) {
        S::project(Range::select::<false, _>(self));
    }
}

impl<'a, O: regions::End> Selection<'a, storage::Shared<O>> {
    pub(in crate::buffer) fn try_read<const N: usize>(
        source: &'a mut storage::Shared<O>,
    ) -> Result<([u8; N], Self), consume::Error> {
        let prefix = Self::try_new(source, N)?;
        // SAFETY: admission retains this source with N initialized bytes;
        // copying the prefix leaves its exclusive owner and extent intact.
        let head = unsafe { *prefix.bytes().as_ptr().cast::<[u8; N]>() };
        Ok((head, prefix))
    }
}

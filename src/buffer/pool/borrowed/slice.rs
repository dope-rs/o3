use pool::{borrowed, state};

use crate::buffer::{self, pool, pool::owners, storage};

/// An exclusive initialized range retaining its original pool and slot.
pub struct Slice<'pool> {
    pub(in crate::buffer::pool::borrowed) span:
        pool::Span<owners::Plain, pool::Borrowed<'pool>, state::Initialized>,
    pub(in crate::buffer::pool::borrowed) start: u32,
    pub(in crate::buffer::pool::borrowed) end: u32,
}

impl buffer::Residency for Slice<'_> {
    fn resident(&self) -> usize {
        self.span.capacity().get() as usize
    }
}

// SAFETY: Slice retains this fully initialized slot's exact selected range.
unsafe impl<'pool> pool::raw::Init for Slice<'pool> {
    type Backing = owners::Plain;
    type Root = pool::Borrowed<'pool>;
    type State = state::Initialized;

    fn into_parts(
        self,
    ) -> (
        pool::Span<Self::Backing, Self::Root, Self::State>,
        usize,
        usize,
    ) {
        let len = self.len();
        (self.span, self.start as usize, len)
    }
}

impl<'pool> Slice<'pool> {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: this exclusive initialized span owns the checked visible range.
        unsafe { buffer::raw::range(self.span.entry().data(), self.start as usize, self.len()) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: no other writable capability survives this slot's acquisition.
        unsafe { buffer::raw::range_mut(self.span.entry().data(), self.start as usize, self.len()) }
    }

    pub fn try_frame(
        self,
        prefix: &[u8],
        suffix: usize,
    ) -> Result<borrowed::Frame<'pool>, (Self, borrowed::Error)> {
        let spare = self.start;
        if prefix.len() > spare as usize {
            return Err((
                self,
                borrowed::Error::Prefix {
                    requested: prefix.len(),
                    available: spare,
                },
            ));
        }
        let spare = self.span.capacity().get() - self.end;
        if suffix > spare as usize {
            return Err((
                self,
                borrowed::Error::Suffix {
                    requested: suffix,
                    available: spare,
                },
            ));
        }
        let start = self.start - prefix.len() as u32;
        // SAFETY: the first admission proves this exact source fits the exclusive slot.
        unsafe {
            buffer::raw::SliceExt::copy_to(prefix, self.span.entry().data().add(start as usize));
        }
        Ok(borrowed::Frame {
            start,
            end: self.end + suffix as u32,
            payload: self,
        })
    }

    pub fn freeze(self) -> storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>> {
        pool::raw::Init::into_shared(self)
    }
}

const _: () = {
    assert!(size_of::<Slice<'static>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Slice<'static>>>() == size_of::<Slice<'static>>());
};

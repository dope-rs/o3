use pool::state;

use crate::buffer::{pool, pool::owners, write::consume::Error};

/// A measured source admitted by the exclusively borrowed destination.
#[must_use = "commit publishes the admitted source"]
pub struct Append<'a, I, T> {
    pub(in crate::buffer::pool) target: &'a mut T,
    pub(in crate::buffer::pool) input: owners::Measured<I>,
}

impl<'a, I, L, M> Append<'a, I, pool::Cursor<L, M>> {
    pub(in crate::buffer::pool) fn try_input(
        target: &'a mut pool::Cursor<L, M>,
        input: owners::Measured<I>,
    ) -> Result<Self, Error> {
        Error::check(input.len(), target.spare_len())?;
        target.compact_for(input.len());
        Ok(Self { target, input })
    }

    pub(in crate::buffer::pool) fn copy(self)
    where
        I: crate::buffer::raw::SliceExt,
    {
        Append {
            target: &mut self.target.lease,
            input: self.input,
        }
        .copy();
    }
}

impl<L, M> Append<'_, &[u8], pool::Cursor<L, M>> {
    /// Publishes the admitted source without checking its bounds again.
    pub fn commit(self) {
        self.copy();
    }
}

impl<'a, 'source, L, M> Append<'a, &'source [u8], pool::Cursor<L, M>> {
    pub(in crate::buffer::pool) fn prefix(
        target: &'a mut pool::Cursor<L, M>,
        src: &'source [u8],
    ) -> Self {
        let len = src.len().min(target.spare_len());
        let input = owners::Measured::slice(&src[..len]);
        target.compact_for(len);
        Self { target, input }
    }
}

const _: () = {
    assert!(size_of::<Append<'_, &[u8], pool::Cursor>>() == 4 * size_of::<usize>());
    assert!(
        size_of::<Append<'_, &[u8], pool::Cursor<pool::Layout, pool::Borrowed<'_>>>>()
            == 4 * size_of::<usize>()
    );
};

impl<'a, I, L, M> Append<'a, I, pool::Span<owners::Plain<L>, M, state::Uninit>> {
    pub(in crate::buffer::pool) fn copy(self) -> &'a mut pool::Span<owners::Plain<L>, M>
    where
        I: crate::buffer::raw::SliceExt,
    {
        let target = self.target.entry().data();
        let start = self.target.len();
        // SAFETY: admission retains every measured source and its disjoint exclusive suffix.
        unsafe { self.input.input().copy_to(target.add(start)) };
        self.target.len = (start + self.input.len()) as u32;
        self.target
    }

    pub(in crate::buffer::pool) fn try_lease(
        target: &'a mut pool::Span<owners::Plain<L>, M, state::Uninit>,
        input: owners::Measured<I>,
    ) -> Result<Self, Error> {
        let spare = target.capacity().get() as usize - target.len();
        Error::check(input.len(), spare)?;
        Ok(Self { target, input })
    }

    pub fn as_slice(&self) -> &[u8] {
        self.target.as_slice()
    }

    pub fn input(&self) -> &I {
        self.input.input()
    }
}

impl<'a, L, M> Append<'a, &[u8], pool::Span<owners::Plain<L>, M, state::Uninit>> {
    /// Publishes the admitted input and returns the same exclusive lease.
    pub fn commit(self) -> &'a mut pool::Span<owners::Plain<L>, M, state::Uninit> {
        self.copy()
    }
}

impl<'a, 'source, L, M, const P: usize, const N: usize>
    Append<'a, ([u8; P], [&'source [u8]; N]), pool::Span<owners::Plain<L>, M, state::Uninit>>
{
    /// Admits an owned prefix and every source as one indivisible write.
    pub fn try_prefixed(
        target: &'a mut pool::Span<owners::Plain<L>, M, state::Uninit>,
        prefix: [u8; P],
        sources: [&'source [u8]; N],
    ) -> Result<Self, Error> {
        let input = owners::Measured::try_prefixed((prefix, sources))?;
        Self::try_lease(target, input)
    }

    /// Publishes once and returns the original source borrows with their lease.
    pub fn commit(self) -> Sources<'a, 'source, L, M, N> {
        let sources = self.input.input().1;
        (self.copy(), sources)
    }
}

const _: () = {
    assert!(size_of::<Append<'_, &[u8], pool::Span>>() == 4 * size_of::<usize>());
    assert!(size_of::<Append<'_, ([u8; 8], [&[u8]; 4]), pool::Span>>() == 11 * size_of::<usize>());
};

type Sources<'a, 'source, L, M, const N: usize> = (
    &'a mut pool::Span<owners::Plain<L>, M, state::Uninit>,
    [&'source [u8]; N],
);

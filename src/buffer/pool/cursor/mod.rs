use crate::{buffer, buffer::pool::owners};
pub(in crate::buffer::pool) mod fill;
pub(in crate::buffer::pool) mod limit;

use std::ptr;

use pool::state;

use crate::buffer::{
    pool, storage, write,
    write::{SpareExt as _, consume},
};

/// One consumable initialized range retaining its original pool root.
pub struct Cursor<L = pool::Layout, M = pool::Owned> {
    pub(in crate::buffer::pool) lease: pool::Span<owners::Plain<L>, M>,
    head: u32,
}

// SAFETY: Cursor retains the initialized suffix beginning at its admitted head.
unsafe impl<L, M> pool::raw::Init for Cursor<L, M> {
    type Backing = owners::Plain<L>;
    type Root = M;
    type State = state::Uninit;

    fn into_parts(self) -> (pool::Span<owners::Plain<L>, M>, usize, usize) {
        let start = self.head as usize;
        let len = self.lease.len() - start;
        (self.lease, start, len)
    }
}

impl<L, M> Cursor<L, M> {
    pub fn len(&self) -> usize {
        self.lease.len() - self.head as usize
    }

    pub fn spare_len(&self) -> usize {
        self.lease.capacity().get() as usize - self.len()
    }

    fn clear(&mut self) {
        self.head = 0;
        self.lease.truncate(0);
    }

    pub fn spare(&mut self) -> write::Spare<'_> {
        self.compact();
        self.lease.spare()
    }

    pub(in crate::buffer::pool) fn compact_for(&mut self, len: usize) {
        if len > self.lease.capacity().get() as usize - self.lease.len() {
            self.compact();
        }
    }

    fn compact(&mut self) {
        if self.head == 0 {
            return;
        }
        let len = self.len();
        let data = self.lease.as_mut_slice().as_mut_ptr();
        // SAFETY: the initialized suffix lies in this exclusive lease, and
        // ptr::copy permits its overlap with the destination prefix.
        unsafe { ptr::copy(data.add(self.head as usize), data, len) };
        self.head = 0;
        self.lease.len = len as u32;
    }
}

const _: () = {
    assert!(size_of::<Cursor>() == 3 * size_of::<usize>());
    assert!(size_of::<Cursor<pool::Layout, pool::Borrowed<'static>>>() == 3 * size_of::<usize>());
};

impl<L, M> Cursor<L, M> {
    fn advance(prefix: buffer::raw::Selection<'_, Self>) {
        let (target, amount) = prefix.into_parts();
        let head = target.head + amount as u32;
        if head as usize == target.lease.len() {
            target.clear();
        } else {
            target.head = head;
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn try_append<'a, 'source>(
        &'a mut self,
        source: &'source [u8],
    ) -> Result<pool::Append<'a, &'source [u8], Self>, consume::Error> {
        let input = owners::Measured::slice(source);
        pool::Append::try_input(self, input)
    }

    /// Copies the largest source prefix admitted by this cursor.
    pub fn write(&mut self, source: &[u8]) -> usize {
        let append = pool::Append::prefix(self, source);
        let copied = append.input.len();
        append.commit();
        copied
    }
}

impl<L> Cursor<L> {
    pub fn recycle(self) {
        self.lease.recycle();
    }

    #[must_use]
    pub fn freeze(self) -> storage::Shared {
        pool::raw::Init::into_shared(self).into()
    }
}

impl<'pool, L> Cursor<L, pool::Borrowed<'pool>> {
    #[must_use]
    pub fn freeze(self) -> storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'pool>>> {
        pool::raw::Init::into_shared(self)
    }
}

impl<L, M> AsRef<[u8]> for Cursor<L, M> {
    fn as_ref(&self) -> &[u8] {
        // SAFETY: From, advance, clear, and compact keep head within this
        // owner's initialized prefix; append only extends that prefix.
        unsafe { self.lease.as_slice().get_unchecked(self.head as usize..) }
    }
}

impl<L, M> consume::Consume for Cursor<L, M> {
    fn remaining(&self) -> usize {
        self.lease.len() - self.head as usize
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        buffer::raw::Selection::try_new(self, amount).map(Self::advance)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        let selected = buffer::raw::Selection::new(self, max);
        let amount = selected.len();
        Self::advance(selected);
        amount
    }
}

impl<L, M> write::Sink for Cursor<L, M> {
    type Error = consume::Error;

    fn write_slice(&mut self, source: &[u8]) -> Result<(), Self::Error> {
        self.try_append(source)?.commit();
        Ok(())
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        let input = owners::Input::try_measure(slices)?;
        buffer::raw::Copy::copy(pool::Append::try_input(self, input)?);
        Ok(())
    }
}

// SAFETY: this unique cursor cannot change its initialized extent through shared access.
unsafe impl<L, M> buffer::raw::Source for Cursor<L, M> {}

impl<L, M> From<pool::Span<owners::Plain<L>, M, state::Uninit>> for Cursor<L, M> {
    fn from(lease: pool::Span<owners::Plain<L>, M, state::Uninit>) -> Self {
        Self { lease, head: 0 }
    }
}

impl<L, M> AsMut<[u8]> for Cursor<L, M> {
    fn as_mut(&mut self) -> &mut [u8] {
        let head = self.head as usize;
        // SAFETY: the same bounded suffix remains exclusively borrowed here.
        unsafe { self.lease.as_mut_slice().get_unchecked_mut(head..) }
    }
}

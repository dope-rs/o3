use crate::buffer::{
    pool,
    pool::{borrowed, owners},
    raw, storage,
};

/// An initialized frame retaining its payload range for exact rejection.
pub struct Frame<'pool> {
    pub(in crate::buffer::pool::borrowed) payload: borrowed::Slice<'pool>,
    pub(in crate::buffer::pool::borrowed) start: u32,
    pub(in crate::buffer::pool::borrowed) end: u32,
}

impl<'pool> Frame<'pool> {
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn prefix_mut(&mut self) -> &mut [u8] {
        let len = (self.payload.start - self.start) as usize;
        // SAFETY: construction proves this prefix inside the exclusive initialized slot.
        unsafe { raw::range_mut(self.payload.span.entry().data(), self.start as usize, len) }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.len();
        // SAFETY: construction retains both frame bounds with the exact exclusive slot.
        unsafe { raw::range_mut(self.payload.span.entry().data(), self.start as usize, len) }
    }

    pub fn payload_end(&self) -> usize {
        (self.payload.end - self.start) as usize
    }

    pub fn payload_mut(&mut self) -> &mut [u8] {
        self.payload.as_mut_slice()
    }

    pub fn into_slice(self) -> borrowed::Slice<'pool> {
        self.payload
    }

    pub fn join(self) -> borrowed::Slice<'pool> {
        let mut payload = self.payload;
        payload.start = self.start;
        payload.end = self.end;
        payload
    }

    pub fn freeze(self) -> storage::Shared<owners::Edge<owners::Plain, pool::Borrowed<'pool>>> {
        self.join().freeze()
    }
}

const _: () = assert!(size_of::<Frame<'static>>() == 4 * size_of::<usize>());

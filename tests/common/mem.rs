use std::{cell::Cell, marker, ptr};

use o3::{
    alloc,
    mem::{self, Return},
};

pub struct Pinned<'a> {
    pub address: Cell<*const Self>,
    pub drops: super::Counted<&'a Cell<Option<usize>>>,
    pub pin: marker::PhantomPinned,
}

impl Drop for Pinned<'_> {
    fn drop(&mut self) {
        assert_eq!(self.address.get(), ptr::from_ref(self));
    }
}

pub enum Root {}
pub enum Child {}

#[repr(transparent)]
pub struct Edge<W: mem::raw::Counter = u32>(pub mem::raw::Count<W>);

// SAFETY: each test value owns one complete, exclusively driven edge domain.
unsafe impl<W: mem::raw::Counter> mem::raw::Edge<W> for Edge<W> {
    fn count(&self) -> &mem::raw::Count<W> {
        &self.0
    }
}

pub struct Sink<'a> {
    pub returned: &'a Cell<Option<u8>>,
    pub calls: &'a Cell<Option<u8>>,
}

impl Return for Sink<'_> {
    type Item = u8;

    fn return_item(self, item: u8) {
        self.calls
            .set(self.calls.get().and_then(|count| count.checked_add(1)));
        self.returned.set(Some(item));
    }
}

pub struct Target<'a, W: mem::raw::Counter = u32> {
    pub refs: mem::raw::Count<W>,
    pub value: usize,
    pub finished: &'a Cell<Option<usize>>,
}

impl<'a, W: mem::raw::Counter> Target<'a, W> {
    pub fn try_owner(
        finished: &'a Cell<Option<usize>>,
    ) -> Result<mem::raw::Owner<Self, W>, alloc::Exhausted> {
        let target = alloc::BoxExt::try_new(Self {
            refs: mem::raw::Count::one(),
            value: 17,
            finished,
        })?;
        // SAFETY: the allocation transfers its one initialized counted edge.
        Ok(unsafe { mem::raw::Owner::from_raw(ptr::NonNull::from(Box::leak(target))) })
    }
}

// SAFETY: the initialized target retains its own immutable reference domain.
unsafe impl<W: mem::raw::Counter> mem::raw::Edge<W> for Target<'_, W> {
    fn count(&self) -> &mem::raw::Count<W> {
        &self.refs
    }
}

// SAFETY: every admitted owner uses this exact boxed target until its last edge.
unsafe impl<W: mem::raw::Counter> mem::raw::Target<W> for Target<'_, W> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: the terminal edge transfers the complete boxed allocation.
        let target = unsafe { Box::from_raw(pointer.as_ptr()) };
        crate::common::bump(target.finished);
    }
}

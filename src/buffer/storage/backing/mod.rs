use std::{num, ptr};

pub(in crate::buffer) use root::Root;

mod root;

/// Exact byte range branded by its live backing-owner borrow.
/// The private root cannot survive long enough for address reuse to forge a match.
#[derive(Clone, Copy)]
pub struct View<'a, 'd> {
    root: Root<'a, 'd>,
    start: ptr::NonNull<u8>,
    len: num::NonZero<usize>,
}

impl<'d> View<'_, 'd> {
    pub const fn is_empty(&self) -> bool {
        false
    }

    pub const fn len(&self) -> usize {
        self.len.get()
    }

    pub fn resident(&self) -> usize {
        self.root.bytes()
    }

    pub fn contains(&self, other: &View<'_, '_>) -> bool {
        self.root == other.root && contains(self.start, self.len(), other.start, other.len())
    }

    pub(in crate::buffer) fn new<'a, O: Owner<'d>>(
        value: &'a super::regions::Value<O>,
    ) -> Option<View<'a, 'd>>
    where
        'd: 'a,
    {
        let root = value.owner().root()?;
        let bytes = value.as_slice();
        let start = ptr::NonNull::from(bytes).cast();
        let len = num::NonZero::new(bytes.len())?;
        Some(View { root, start, len })
    }
}

pub(in crate::buffer) trait Owner<'d> {
    fn root<'a>(&'a self) -> Option<Root<'a, 'd>>
    where
        'd: 'a;
}

fn contains(base: ptr::NonNull<u8>, span: usize, item: ptr::NonNull<u8>, size: usize) -> bool {
    let base = base.addr().get();
    let item = item.addr().get();
    item >= base && size <= span && item - base <= span - size
}

const _: () = {
    assert!(size_of::<View<'static, 'static>>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<View<'static, 'static>>>() == size_of::<View<'static, 'static>>());
};

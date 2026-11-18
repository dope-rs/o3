use std::{num, ptr};

use crate::buffer::{resident, storage};

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
        self.root.same_registration(other.root)
            && self.start <= other.start
            && other.end() <= self.end()
    }

    fn end(&self) -> *const u8 {
        self.start.as_ptr().wrapping_add(self.len())
    }
}

impl<'a, 'd> Root<'a, 'd> {
    fn view(self, bytes: &'a [u8]) -> Option<View<'a, 'd>> {
        Some(View {
            root: self,
            start: ptr::NonNull::from(bytes).cast(),
            len: num::NonZero::new(bytes.len())?,
        })
    }
}

impl storage::Shared {
    #[doc(hidden)]
    pub fn backing(&self) -> Option<View<'_, '_>> {
        self.owner().as_ref()?.root().view(self.as_slice())
    }
}

impl<'d> resident::Shared<'d> {
    #[doc(hidden)]
    pub fn backing(&self) -> Option<View<'_, 'd>> {
        self.region.owner().root()?.view(self.as_slice())
    }
}

const _: () = {
    assert!(size_of::<View<'static, 'static>>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<View<'static, 'static>>>() == size_of::<View<'static, 'static>>());
};

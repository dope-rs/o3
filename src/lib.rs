#![doc = include_str!("compile_fail.md")]

const _: () = assert!(
    usize::BITS >= 64,
    "o3 requires a 64-bit target: capacities are u32 and index/layout math assumes usize >= u32"
);

pub mod buffer;
pub mod cell;
pub mod collections;
pub mod mem;
pub mod num;

use std::marker;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ThreadBound(marker::PhantomData<*mut ()>);

impl ThreadBound {
    pub const NEW: Self = Self(marker::PhantomData);
}

/// Receives a resource when its owning [`ReturnPermit`] is dropped.
pub trait ReturnTo {
    type Item;

    fn return_item(&self, item: Self::Item);
}

/// Returns its item to the sink when dropped.
#[must_use]
pub struct ReturnPermit<R: ReturnTo> {
    item: Option<R::Item>,
    sink: R,
}

impl<R: ReturnTo> ReturnPermit<R> {
    pub fn new(sink: R, item: R::Item) -> Self {
        Self {
            item: Some(item),
            sink,
        }
    }
}

impl<R: ReturnTo> Drop for ReturnPermit<R> {
    fn drop(&mut self) {
        if let Some(item) = self.item.take() {
            self.sink.return_item(item);
        }
    }
}

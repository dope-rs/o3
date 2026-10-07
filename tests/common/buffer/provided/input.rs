use std::cell::Cell;

use o3::buffer;

use crate::common::buffer::provided::Item;

pub struct Input<'a> {
    pub buffer: Item<'a>,
    pub converted: &'a Cell<Option<usize>>,
}

// SAFETY: conversion moves the sole Item; the returned projector retains its complete bytes.
unsafe impl<'a> buffer::raw::Input<Item<'a>> for Input<'a> {
    fn bytes(&self) -> &[u8] {
        &self.buffer.bytes
    }

    fn into_item(
        self,
    ) -> (
        Item<'a>,
        impl for<'b> FnOnce(&'b mut Item<'a>) -> &'b mut [u8],
    ) {
        self.converted
            .set(self.converted.get().and_then(|count| count.checked_add(1)));
        (self.buffer, Item::bytes_mut)
    }
}

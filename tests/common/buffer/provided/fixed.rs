use std::num::NonZero;

use o3::buffer;

use crate::common::buffer::provided::Item;

#[derive(Debug)]
pub struct Fixed {
    pub len: usize,
    pub resident: NonZero<u32>,
}

impl buffer::raw::Fixed<Item<'_>> for Fixed {
    fn resident(&self) -> NonZero<u32> {
        self.resident
    }

    unsafe fn project<'a, 'id>(
        &'a self,
        item: &'a mut Item<'_>,
        limit: buffer::raw::Limit<'id>,
    ) -> buffer::raw::Projection<'a, 'id> {
        // SAFETY: admission supplies this context's initialized prefix within
        // both the original item's extent and its fixed resident ceiling.
        unsafe { limit.project(item.bytes_mut().get_unchecked_mut(..self.len)) }
    }
}

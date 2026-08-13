use std::{pin, ptr};

use o3::cell::raw;

use crate::primitives::cells::StableValue;

pub(super) struct StableSource<'a>(pub(super) pin::Pin<&'a StableValue>);

unsafe impl raw::StableLinkSource<StableValue> for StableSource<'_> {
    fn pointer(self) -> ptr::NonNull<StableValue> {
        ptr::NonNull::from(self.0.get_ref())
    }
}

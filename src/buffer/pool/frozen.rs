use std::{marker::PhantomData, ptr::NonNull};

use crate::buffer::pool::core::Core;

pub struct Frozen {
    pub(super) core: NonNull<Core>,
    pub(super) index: u32,
    pub(super) len: u32,
    pub(super) marker: PhantomData<*mut ()>,
}

impl Frozen {
    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        Core::slice(self.core, self.index, self.len())
    }
}

impl Clone for Frozen {
    fn clone(&self) -> Self {
        Core::retain_slot(self.core, self.index);
        Self {
            core: self.core,
            index: self.index,
            len: self.len,
            marker: PhantomData,
        }
    }
}

impl AsRef<[u8]> for Frozen {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl Drop for Frozen {
    fn drop(&mut self) {
        Core::release_slot(self.core, self.index);
    }
}

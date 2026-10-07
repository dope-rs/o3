use std::mem;

use runtime::entry;

use crate::{
    base::num::bounded,
    depot::arena::hash::{self, runtime},
};

pub(in crate::depot) struct Slot<'a, T, Tag> {
    core: &'a mut hash::Core<T, Tag>,
    pub(in crate::depot) index: bounded::U32<0, { u32::MAX - 1 }>,
}

impl<'a, T, Tag> Slot<'a, T, Tag> {
    pub(in crate::depot) fn new(core: &'a mut hash::Core<T, Tag>) -> Option<Self> {
        let index = bounded::U32::from_one(core.free?.position());
        Some(Self { core, index })
    }

    pub(in crate::depot) fn insert(self, value: T) -> entry::Live<'a, T, Tag, entry::Staged> {
        let core = self.core;
        let position = core.len;
        let next = unsafe { core.meta(self.index) }.next();
        unsafe {
            core.values
                .as_ptr()
                .add(self.index.into_usize())
                .write(mem::MaybeUninit::new(value));
            core.dense.as_ptr().add(position as usize).write(self.index);
            let meta = core.meta_mut(self.index);
            meta.slot = position;
            meta.bucket = hash::Bucket::NONE;
        }
        core.free = next;
        core.len += 1;
        unsafe { hash::Entry::new(core, self.index) }.with_phase()
    }
}

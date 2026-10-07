use crate::{
    buffer::{self, storage::regions::Tag},
    mem::{self, rc},
};

// SAFETY: each plain backing stores its exact Count at offset zero.
unsafe impl<M> mem::raw::Edge for Tag<M> {
    fn count(&self) -> &mem::raw::Count {
        // SAFETY: the source edge keeps its initialized first field live.
        unsafe { self.untag().cast().as_ref() }
    }
}

// SAFETY: the concrete edge retains its original allocation, ownership, and lifetime.
unsafe impl<M> buffer::raw::Retain for Tag<M> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        self.try_clone()
    }
}

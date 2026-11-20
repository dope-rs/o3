use storage::provided;

use crate::{
    buffer,
    buffer::{
        storage,
        storage::{provided::arena, regions},
    },
    mem::{raw, rc},
};

/// One edge retaining its concrete initialized registration and scope.
#[derive(Debug)]
pub struct Account<'d> {
    pub(super) registration: raw::Owner<arena::entry::Registration<'d>>,
}

impl<'d> Account<'d> {
    pub(in crate::buffer) fn capacity(&self) -> &usize {
        // SAFETY: this counted edge was issued from the complete slot and keeps its ceiling immutable.
        unsafe {
            self.registration
                .as_ptr()
                .cast::<arena::Entry<'d>>()
                .as_ref()
                .capacity()
        }
    }
}

// SAFETY: the concrete edge retains its original allocation, ownership, and lifetime.
unsafe impl<'d> buffer::raw::Retain for Account<'d> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            registration: self.registration.try_clone()?,
        })
    }
}

impl regions::End for Option<Account<'_>> {
    const EMPTY: Option<Self> = Some(None);
}

const _: () = assert!(size_of::<Account<'_>>() == size_of::<usize>());

impl buffer::Residency for Option<provided::Account<'_>> {
    fn resident(&self) -> usize {
        self.as_ref().map_or(0, |owner| *owner.capacity())
    }
}

use storage::{backing, provided};

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
    pub(super) registration: raw::Owner<arena::Registration<'d>>,
}

impl<'d> Account<'d> {
    pub(in crate::buffer) fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            registration: self.registration.try_clone()?,
        })
    }
    pub(in crate::buffer) fn charge(&self) -> &buffer::resident::Charge<'d> {
        // SAFETY: the owner borrow retains this exact initialized registration.
        unsafe { (*self.registration.get().value.get()).assume_init_ref() }
    }
}

impl regions::End for Option<Account<'_>> {
    const EMPTY: Option<Self> = Some(None);
}

const _: () = assert!(size_of::<Account<'_>>() == size_of::<usize>());

impl<'d> backing::Owner<'d> for Account<'d> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        Some(backing::Root::Provided(self.charge()))
    }
}

impl buffer::Residency for Option<provided::Account<'_>> {
    fn resident(&self) -> usize {
        self.as_ref().map_or(0, |owner| owner.charge().capacity())
    }
}

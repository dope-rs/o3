use storage::{backing, provided};

use crate::{
    buffer,
    buffer::{pool::owners, resident, storage, storage::regions},
    mem::rc,
};

/// Provenance of one immutable extent; constructors remain inside buffer.
#[derive(Debug)]
#[repr(usize)]
pub(in crate::buffer) enum State<'d> {
    Slice,
    Owned(regions::Tag),
    Static,
    Resident(crate::buffer::raw::Rc<resident::Budget<'d>>),
    Accounted(owners::Registered<'d>),
    Provided(provided::Account<'d>),
}

impl buffer::Residency for State<'_> {
    #[inline]
    fn resident(&self) -> usize {
        use backing::Owner as _;
        self.root().map_or(0, backing::Root::bytes)
    }
}

// SAFETY: every variant retains the same backing identity and original domain.
unsafe impl buffer::raw::Retain for State<'_> {
    #[inline]
    fn try_retain(&self) -> Result<Self, rc::Error> {
        Ok(match self {
            Self::Slice => Self::Slice,
            Self::Owned(owner) => Self::Owned(owner.try_retain()?),
            Self::Static => Self::Static,
            Self::Resident(owner) => Self::Resident(owner.try_retain()?),
            Self::Accounted(owner) => Self::Accounted(owner.try_retain()?),
            Self::Provided(owner) => Self::Provided(owner.try_retain()?),
        })
    }
}

impl<'d> backing::Owner<'d> for State<'d> {
    #[inline]
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        match self {
            Self::Slice | Self::Static => None,
            Self::Owned(owner) => Some(owner.root()),
            Self::Resident(owner) => owner.root(),
            Self::Accounted(owner) => owner.root(),
            Self::Provided(owner) => owner.root(),
        }
    }
}
const _: () = assert!(size_of::<State<'static>>() == 2 * size_of::<usize>());

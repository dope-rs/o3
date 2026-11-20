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
    Pool(owners::Owner<owners::Charged<'d>, buffer::pool::Owned>),
}

impl buffer::Residency for State<'_> {
    #[inline]
    fn resident(&self) -> usize {
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
            Self::Pool(owner) => Self::Pool(owner.try_retain()?),
        })
    }
}

impl<'d> State<'d> {
    #[inline]
    pub(in crate::buffer) fn root(&self) -> Option<backing::Root<'_, 'd>> {
        use backing::Root;

        Some(match self {
            Self::Slice | Self::Static => return None,
            Self::Owned(owner) => owner.root(),
            Self::Resident(owner) => Root::Resident(owner.get()),
            Self::Accounted(owner) => Root::Accounted(owner.get()),
            Self::Provided(owner) => Root::Provided(owner.capacity()),
            Self::Pool(owner) => owner.root(),
        })
    }
}
const _: () = assert!(size_of::<State<'static>>() == 2 * size_of::<usize>());

use storage::{backing, provided};

use crate::{
    buffer,
    buffer::{pool, pool::owners, resident, storage, storage::regions},
    mem::rc,
};

/// Provenance of one immutable extent; constructors remain inside buffer.
#[derive(Debug)]
#[repr(u8)]
pub(in crate::buffer) enum State<'d> {
    Slice,
    Owned(Option<regions::Tag>),
    Borrowed(regions::Tag<pool::Borrowed<'d>>),
    Resident(crate::buffer::raw::Rc<resident::Lease<'d>>),
    Accounted(owners::Registered<'d>),
    Provided(provided::Account<'d>),
}

// SAFETY: every variant retains the same backing identity and original domain.
unsafe impl buffer::raw::Retain for State<'_> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        Ok(match self {
            Self::Slice => Self::Slice,
            Self::Owned(owner) => Self::Owned(owner.try_retain()?),
            Self::Borrowed(owner) => Self::Borrowed(owner.try_retain()?),
            Self::Resident(owner) => Self::Resident(owner.try_retain()?),
            Self::Accounted(owner) => Self::Accounted(owner.try_retain()?),
            Self::Provided(owner) => Self::Provided(owner.try_retain()?),
        })
    }
}

impl<'d> State<'d> {
    pub(in crate::buffer) fn try_own(self, empty: bool) -> Result<Option<regions::Tag>, Self> {
        match self {
            Self::Owned(owner) => Ok(owner),
            Self::Slice if empty => Ok(None),
            Self::Borrowed(owner) => Ok(Some(owner.into_owned())),
            Self::Slice | Self::Resident(_) | Self::Accounted(_) | Self::Provided(_) => Err(self),
        }
    }
    pub(in crate::buffer) fn resident(&self) -> usize {
        use backing::Owner as _;
        self.root().map_or(0, backing::Root::bytes)
    }
}
impl<'d> backing::Owner<'d> for State<'d> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        match self {
            Self::Slice => None,
            Self::Owned(owner) => owner.root(),
            Self::Borrowed(owner) => Some(owner.root()),
            Self::Resident(owner) => owner.root(),
            Self::Accounted(owner) => owner.root(),
            Self::Provided(owner) => owner.root(),
        }
    }
}
const _: () = assert!(size_of::<State<'static>>() == 2 * size_of::<usize>());

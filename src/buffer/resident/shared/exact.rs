use crate::{
    buffer,
    buffer::{resident, storage, storage::regions},
    mem::rc,
};

/// One-word exact allocation owner bounded by a resident domain lifetime.
#[derive(Debug, Default)]
#[repr(transparent)]
pub struct Exact<'d> {
    owner: Option<regions::Complete<'d>>,
}

impl<'d> Exact<'d> {
    #[must_use]
    pub fn from_plain(plain: storage::Exact) -> Self {
        Self {
            owner: plain.inner.map(regions::Init::<()>::bind),
        }
    }

    pub(in crate::buffer) fn from_init(inner: regions::Init<resident::Lease<'d>>) -> Self {
        Self {
            owner: Some(inner.bind()),
        }
    }

    #[must_use]
    pub const fn new() -> Self {
        Self { owner: None }
    }

    pub fn try_copy(
        budget: &resident::Budget<'d>,
        bytes: &[u8],
    ) -> Result<Self, resident::AllocError> {
        Self::try_slices(budget, [bytes])
    }

    pub fn try_slices<const N: usize>(
        budget: &resident::Budget<'d>,
        slices: [&[u8]; N],
    ) -> Result<Self, resident::AllocError> {
        let inner = regions::Init::<resident::Lease<'d>>::try_copy(budget, slices)?;
        Ok(inner.map_or_else(Self::new, Self::from_init))
    }

    #[must_use]
    pub fn into_shared(self) -> resident::Shared<'d> {
        self.owner
            .map_or_else(resident::Shared::new, regions::Complete::into_buf)
    }

    pub fn bytes(&self) -> usize {
        self.owner.as_ref().map_or(0, regions::Complete::bytes)
    }

    pub fn as_slice(&self) -> &[u8] {
        self.owner.as_ref().map_or(&[], regions::Complete::as_slice)
    }
}

impl Exact<'_> {
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            owner: self
                .owner
                .as_ref()
                .map(regions::Complete::try_clone)
                .transpose()?,
        })
    }
}

const _: () = assert!(size_of::<Exact<'static>>() == size_of::<usize>());

impl AsRef<[u8]> for Exact<'_> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

// SAFETY: exact owners expose one immutable initialized allocation until dropped.
unsafe impl buffer::raw::Source for Exact<'_> {}

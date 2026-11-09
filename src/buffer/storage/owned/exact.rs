use crate::{
    buffer,
    buffer::{storage, storage::regions, write::spans},
};

/// Lifetime-independent one-word exact allocation owner.
#[derive(Debug, Default)]
#[repr(transparent)]
pub struct Exact {
    pub(in crate::buffer) inner: Option<regions::Init>,
}

impl Exact {
    #[must_use]
    pub const fn new() -> Self {
        Self { inner: None }
    }

    pub fn try_build<E>(
        capacity: usize,
        build: impl FnOnce(&mut spans::Writer<'_, '_>) -> Result<(), E>,
    ) -> Result<Self, storage::BuildError<E>> {
        let owned = storage::Owned::try_build(capacity, build)?;
        spans::Gap::check(capacity as u32, owned.len)
            .map_err(|error| storage::BuildError::Length(error.into()))?;
        Ok(Self {
            // SAFETY: try_build preserves admitted capacity; Gap proves full initialization.
            inner: owned
                .allocation
                .map(|heap| unsafe { regions::Init::new(heap) }),
        })
    }

    pub fn try_copy(bytes: &[u8]) -> Result<Self, storage::AllocError> {
        use storage::AllocError;

        let Some(slices) = buffer::raw::Slices::try_new([bytes]).map_err(AllocError::Capacity)?
        else {
            return Ok(Self::new());
        };
        slices.try_owned().map_err(AllocError::Exhausted)
    }

    #[must_use]
    pub fn into_shared(self) -> storage::Shared {
        match self.inner {
            Some(inner) => inner.into_buf().into(),
            None => storage::Shared::new(),
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.inner.as_ref().map_or(&[], regions::Init::as_slice)
    }
}

impl AsRef<[u8]> for Exact {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl buffer::Residency for Exact {
    fn resident(&self) -> usize {
        self.inner
            .as_ref()
            .map_or(0, |inner| inner.capacity().get() as usize)
    }
}

const _: () = assert!(size_of::<Exact>() == size_of::<usize>());

// SAFETY: the exact owner exposes its complete initialized allocation immutably.
unsafe impl buffer::raw::Source for Exact {}

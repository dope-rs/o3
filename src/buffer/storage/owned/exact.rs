use crate::{
    buffer,
    buffer::{
        storage,
        storage::regions,
        write::{SpareExt as _, consume, spans},
    },
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
        build: impl FnOnce(&mut spans::Writer<'_, '_, true>) -> Result<(), E>,
    ) -> Result<Self, storage::BuildError<E>> {
        use storage::BuildError;

        let mut owned = storage::Owned::try_with(capacity)?;
        let mut spare = owned.spare();
        let mut exact = spare.exact();
        build(&mut exact).map_err(BuildError::Build)?;
        consume::Error::check(exact.cursor.limit() as usize, exact.cursor.len() as usize)
            .map_err(BuildError::Length)?;
        exact.cursor.commit();
        spare.commit();
        let Some(heap) = owned.bytes.allocation else {
            return Ok(Self::new());
        };
        // SAFETY: the exact span completed this owner's entire uninitialized suffix.
        Ok(Self {
            inner: Some(unsafe { regions::Init::new(heap.into_shared()) }),
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

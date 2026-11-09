use crate::buffer;
pub(in crate::buffer::storage) mod bytes;
use std::num;

use storage::alloc;

use crate::buffer::{
    bytes::bounded,
    resident, storage,
    write::{SpareExt as _, Staging, consume, spans},
};

pub(super) mod exact;
pub(super) mod nonempty;

/// Construction and ownership transitions for one initialized byte allocation.
#[derive(Debug)]
#[repr(C)]
pub struct Owned<P: crate::buffer::raw::Policy = ()> {
    pub(in crate::buffer::storage) allocation: Option<crate::buffer::raw::Heap<P>>,
    pub(in crate::buffer::storage) len: u32,
}

impl Owned {
    pub fn try_with(capacity: usize) -> Result<Self, storage::AllocError> {
        use storage::AllocError;
        consume::Error::check(capacity, u32::MAX as usize).map_err(AllocError::Capacity)?;
        let capacity = capacity as u32;
        let Some(capacity) = num::NonZero::<u32>::new(capacity) else {
            return Ok(Self::new());
        };
        let heap = crate::buffer::raw::Heap::try_new((), capacity)
            .map_err(alloc::Error::into_alloc)
            .map_err(AllocError::Exhausted)?;
        Ok(Self::from_heap(Some(heap)))
    }

    /// Reserves a total capacity while preserving the initialized bytes.
    /// Smaller targets leave the allocation unchanged.
    pub fn try_reserve(
        &mut self,
        target: num::NonZero<u32>,
    ) -> Result<(), crate::alloc::Exhausted> {
        match &mut self.allocation {
            Some(heap) => heap.try_reserve(target),
            None => crate::buffer::raw::Heap::try_new((), target)
                .map(|heap| self.allocation = Some(heap)),
        }
        .map_err(alloc::Error::into_alloc)
    }

    pub fn try_build<E>(
        capacity: usize,
        build: impl FnOnce(&mut spans::Writer<'_, '_>) -> Result<(), E>,
    ) -> Result<Self, storage::BuildError<E>> {
        use storage::BuildError;
        let mut value = Self::try_with(capacity)?;
        let mut spare = value.spare();
        let mut writer = spare.bounded();
        build(&mut writer).map_err(BuildError::Build)?;
        writer.commit();
        value.len = spare.len() as u32;
        Ok(value)
    }

    pub fn try_filled(len: usize, byte: u8) -> Result<Self, storage::AllocError> {
        let mut value = Self::try_with(len)?;
        let mut spare = value.spare();
        spare.fill(byte);
        spare.commit();
        Ok(value)
    }
}

impl<P: crate::buffer::raw::Policy> Owned<P> {
    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), consume::Error> {
        Staging::try_new(self, [src]).map(Staging::copy)
    }
}

impl Owned {
    #[must_use]
    #[inline]
    pub fn freeze(self) -> storage::Shared {
        self.into_shared().map(Into::into).unwrap_or_default()
    }

    #[must_use]
    pub fn new() -> Self {
        Self::from_heap(None)
    }
}

impl<P: crate::buffer::raw::Policy> AsRef<[u8]> for Owned<P> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<P: crate::buffer::raw::Policy> AsMut<[u8]> for Owned<P> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl<P: crate::buffer::raw::Policy> Owned<P> {
    pub(in crate::buffer) fn into_shared(
        self,
    ) -> Option<storage::Shared<crate::buffer::raw::Rc<P>>> {
        let heap = self.allocation?;
        let len = num::NonZero::new(self.len)?;
        // SAFETY: Owned retains this live allocation's initialized prefix.
        Some(unsafe { heap.into_shared().into_range(0, len.get()) })
    }

    pub(in crate::buffer) const EMPTY: Self = Self::from_heap(None);

    pub(in crate::buffer) const fn from_heap(heap: Option<crate::buffer::raw::Heap<P>>) -> Self {
        Self {
            allocation: heap,
            len: 0,
        }
    }
}

impl<'d> Owned<resident::Budget<'d>> {
    pub fn try_compact(&mut self) -> Result<(), crate::alloc::Exhausted> {
        let Some(heap) = &mut self.allocation else {
            return Ok(());
        };
        let Some(capacity) = num::NonZero::new(self.len) else {
            *self = Self::EMPTY;
            return Ok(());
        };
        heap.try_compact(capacity)
    }
}

const _: () = assert!(size_of::<Owned<()>>() == 2 * size_of::<usize>());

/// Unique bytes charged to one resident domain.
impl<'d> storage::Owned<resident::Budget<'d>> {
    pub fn try_copy(
        source: &impl bounded::Source,
        budget: &resident::Budget<'d>,
    ) -> Result<Self, resident::copy::Error> {
        let Some(source) = buffer::raw::Slices::from_source(source) else {
            return Ok(Self::EMPTY);
        };
        Ok(source.try_init(*budget)?.into_owned())
    }

    #[inline]
    pub fn try_from_slice(
        budget: &resident::Budget<'d>,
        bytes: &[u8],
    ) -> Result<Self, resident::AllocError> {
        let Some(source) =
            buffer::raw::Slices::try_new([bytes]).map_err(resident::AllocError::Capacity)?
        else {
            return Ok(Self::EMPTY);
        };
        Ok(source.try_init(*budget)?.into_owned())
    }

    /// Allocates against an existing debit without acquiring resident capacity again.
    /// Failure releases the consumed charge; a zero charge needs no allocation.
    pub fn try_from_charge(charge: resident::Charge<'d>) -> Result<Self, storage::AllocError> {
        let storage = crate::buffer::raw::Debit::try_from_charge(charge)
            .map_err(storage::AllocError::Capacity)?
            .map(crate::buffer::raw::Heap::try_charged)
            .transpose()
            .map_err(storage::AllocError::Exhausted)?;
        Ok(Self::from_heap(storage))
    }

    pub fn try_allocate(
        budget: &resident::Budget<'d>,
        capacity: usize,
    ) -> Result<Self, resident::AllocError> {
        consume::Error::check(capacity, u32::MAX as usize)
            .map_err(resident::AllocError::Capacity)?;
        let capacity = capacity as u32;
        let storage = num::NonZero::<u32>::new(capacity)
            .map(|capacity| crate::buffer::raw::Heap::try_new(*budget, capacity))
            .transpose()?;
        Ok(Self::from_heap(storage))
    }

    /// Reserves a total capacity while preserving the initialized bytes.
    /// `seed` is used only without an allocation; growth retains the original domain.
    pub fn try_reserve(
        &mut self,
        seed: &resident::Budget<'d>,
        target: usize,
    ) -> Result<(), resident::AllocError> {
        consume::Error::check(target, u32::MAX as usize).map_err(resident::AllocError::Capacity)?;
        let Some(target) = num::NonZero::new(target as u32) else {
            return Ok(());
        };
        match &mut self.allocation {
            Some(heap) => heap.try_reserve(target)?,
            None => self.allocation = Some(crate::buffer::raw::Heap::try_new(*seed, target)?),
        }
        Ok(())
    }

    #[must_use]
    #[inline]
    pub fn freeze(self) -> resident::Shared<'d> {
        self.into_shared().map(Into::into).unwrap_or_default()
    }
}

const _: () =
    assert!(size_of::<storage::Owned<resident::Budget<'static>>>() == size_of::<usize>() * 2);

impl<P: crate::buffer::raw::Policy> Default for Owned<P> {
    fn default() -> Self {
        Self::EMPTY
    }
}

// SAFETY: shared access cannot change the unique allocation or initialized range.
unsafe impl<P: crate::buffer::raw::Policy> buffer::raw::Source for Owned<P> {}

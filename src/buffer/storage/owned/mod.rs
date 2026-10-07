use crate::{buffer, buffer::storage::regions};
pub(in crate::buffer::storage) mod bytes;
use std::{num, ops};

use storage::{Bytes, alloc};

use crate::buffer::{
    resident, storage,
    write::{SpareExt as _, Staging, consume, spans},
};

pub(super) mod exact;
pub(super) mod nonempty;

/// Construction and ownership transitions for one initialized byte allocation.
#[derive(Debug)]
#[repr(transparent)]
pub struct Owned<P: crate::buffer::raw::Policy = (), const CAP: u32 = 0> {
    pub(in crate::buffer::storage) bytes: Bytes<P, CAP>,
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> ops::Deref for Owned<P, CAP> {
    type Target = Bytes<P, CAP>;
    fn deref(&self) -> &Self::Target {
        &self.bytes
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> ops::DerefMut for Owned<P, CAP> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.bytes
    }
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

    pub fn try_build<E>(
        capacity: usize,
        build: impl FnOnce(&mut spans::Writer<'_, '_, false>) -> Result<(), E>,
    ) -> Result<Self, storage::BuildError<E>> {
        use storage::BuildError;
        let mut value = Self::try_with(capacity)?;
        let mut spare = value.spare();
        let mut writer = spare.bounded();
        build(&mut writer).map_err(BuildError::Build)?;
        writer.commit();
        spare.commit();
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

impl<P: crate::buffer::raw::Policy, const CAP: u32> Owned<P, CAP> {
    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), consume::Error> {
        Staging::try_new(&mut self.bytes, [src]).map(buffer::raw::Copy::copy)
    }
}

impl<const CAP: u32> Owned<(), CAP> {
    #[must_use]
    pub fn freeze(self) -> storage::Shared {
        let Some(buf) = self.into_shared() else {
            return storage::Shared::empty(None);
        };
        buf.into()
    }
}

impl Owned {
    #[must_use]
    pub fn new() -> Self {
        Self::from_heap(None)
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> AsRef<[u8]> for Owned<P, CAP> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> AsMut<[u8]> for Owned<P, CAP> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl<P: crate::buffer::raw::Policy, const CAP: u32> Owned<P, CAP> {
    pub(in crate::buffer) fn try_init(self) -> Result<Option<regions::Init<P>>, Self> {
        if self.len() != self.capacity() {
            return Err(self);
        }
        match self.bytes.allocation {
            None => Ok(None),
            Some(heap) => {
                // SAFETY: the initialized length equals this live allocation's capacity.
                Ok(Some(unsafe { regions::Init::new(heap.into_shared()) }))
            }
        }
    }

    pub(in crate::buffer) fn into_shared(
        self,
    ) -> Option<storage::Shared<crate::buffer::raw::Rc<P>>> {
        let heap = self.bytes.allocation?;
        let len = num::NonZero::new(self.bytes.len)?;
        // SAFETY: Owned retains this live allocation's initialized prefix.
        Some(unsafe { heap.into_shared().into_range(0, len.get()) })
    }
}

impl<P: crate::buffer::raw::Policy> Owned<P> {
    pub(in crate::buffer) const EMPTY: Self = Self {
        bytes: Bytes {
            allocation: None,
            len: 0,
        },
    };

    pub(in crate::buffer) fn from_heap(heap: Option<crate::buffer::raw::Heap<P>>) -> Self {
        Self {
            bytes: Bytes {
                allocation: heap,
                len: 0,
            },
        }
    }
}

impl<'d> Owned<resident::Lease<'d>> {
    pub fn try_compact(&mut self) -> Result<(), crate::alloc::Exhausted> {
        let Some(heap) = &mut self.bytes.allocation else {
            return Ok(());
        };
        let Some(capacity) = num::NonZero::new(self.bytes.len) else {
            *self = Self::EMPTY;
            return Ok(());
        };
        heap.try_compact(capacity)
    }
}

impl<const CAP: u32> Owned<(), CAP> {
    pub fn try_new() -> Result<Self, crate::alloc::Exhausted> {
        let state = match num::NonZero::<u32>::new(CAP) {
            None => None,
            Some(capacity) => Some(
                crate::buffer::raw::Heap::try_new((), capacity)
                    .map_err(alloc::Error::into_alloc)?,
            ),
        };
        Ok(Self {
            bytes: Bytes {
                allocation: state,
                len: 0,
            },
        })
    }
}

const _: () = {
    assert!(size_of::<Owned<()>>() == 2 * size_of::<usize>());
    assert!(size_of::<Owned<(), 1>>() == size_of::<Owned<()>>());
};

/// Unique bytes charged to one resident domain.
impl<'d> storage::Owned<resident::Lease<'d>> {
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
        match &mut self.bytes.allocation {
            Some(heap) => heap.try_reserve(target)?,
            None => self.bytes.allocation = Some(crate::buffer::raw::Heap::try_new(*seed, target)?),
        }
        Ok(())
    }

    #[must_use]
    pub fn freeze(self) -> resident::Shared<'d> {
        let Some(inner) = self.into_shared() else {
            return resident::Shared::new();
        };
        inner.into()
    }

    pub fn try_freeze(self) -> Result<resident::Exact<'d>, Self> {
        Ok(match self.try_init()? {
            Some(inner) => resident::Exact::from_init(inner),
            None => resident::Exact::new(),
        })
    }
}

const _: () =
    assert!(size_of::<storage::Owned<resident::Lease<'static>>>() == size_of::<usize>() * 2);

impl Default for Owned {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: shared access cannot change the unique allocation or initialized range.
unsafe impl<P: crate::buffer::raw::Policy, const CAP: u32> buffer::raw::Source for Owned<P, CAP> {}

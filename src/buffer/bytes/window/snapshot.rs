use std::{mem, num, slice};

use crate::{
    buffer,
    buffer::{
        bytes::window,
        pool::owners,
        storage,
        write::{self, Sink, consume, consume::Consume},
    },
    mem::rc,
};

/// Growable byte accumulator bounded by `MAX_CAPACITY`.
#[derive(Default)]
pub struct Snapshot<const MAX_CAPACITY: usize> {
    state: Option<window::State<(), MAX_CAPACITY>>,
    tail: u32,
}

impl<const MAX_CAPACITY: usize> Snapshot<MAX_CAPACITY> {
    /// Creates an empty accumulator without allocating.
    pub const fn new() -> Self {
        Self {
            state: None,
            tail: 0,
        }
    }

    pub fn try_with(capacity: num::NonZero<usize>) -> Result<Self, storage::AllocError> {
        Self::try_copy(&[], capacity)
    }

    /// Copies the input into one allocation, growing from the initial capacity as needed.
    pub fn try_copy(
        src: &[u8],
        capacity: num::NonZero<usize>,
    ) -> Result<Self, storage::AllocError> {
        let state = window::Fresh::try_copy(src, capacity)?;
        Ok(Self {
            state: Some(state),
            tail: 0,
        })
    }

    /// Borrows the physical unpublished suffix without allocating or reclaiming bytes.
    pub fn spare(&mut self) -> write::Spare<'_> {
        let Some(state) = &mut self.state else {
            // SAFETY: an absent allocation has no initialized or writable bytes.
            return unsafe { write::Spare::from_parts(&mut [], &mut self.tail) };
        };
        let pointer = state.buf.data().cast_mut().cast::<mem::MaybeUninit<u8>>();
        let start = state.head as usize;
        let len = (state.cap.get() - state.head) as usize;
        // SAFETY: head excludes every published snapshot. This exclusive borrow
        // keeps the disjoint unpublished suffix and its initialization word live.
        let spare = unsafe { slice::from_raw_parts_mut(pointer.add(start), len) };
        // SAFETY: the same allocation owns this suffix and its unchanged u32 head.
        unsafe { write::Spare::from_parts(spare, &mut state.head) }
    }

    /// Transfers the visible suffix without retaining another reference.
    pub fn into_shared(self) -> Option<storage::Shared> {
        if self.is_empty() {
            return None;
        }
        Some(self.freeze())
    }

    /// Transfers the initialized suffix, retaining its allocation even when empty.
    pub fn freeze(self) -> storage::Shared {
        let Some(state) = self.state else {
            return storage::Shared::new();
        };
        let len = state.head - self.tail;
        // SAFETY: Snapshot transfers its exact initialized suffix and owning edge;
        // a zero-length suffix at the initialized end is a valid owned extent.
        storage::Shared::from(unsafe { state.buf.into_range(self.tail, len) })
    }

    pub fn try_snapshot(&self) -> Result<Option<storage::Shared>, rc::Error> {
        use buffer::raw::Retain as _;

        let Some(state) = &self.state else {
            return Ok(None);
        };
        if state.head == self.tail {
            return Ok(None);
        }
        let len = state.head - self.tail;
        // SAFETY: Snapshot keeps this exact initialized suffix within its allocation.
        Ok(Some(storage::Shared::from(unsafe {
            state.buf.try_retain()?.into_range(self.tail, len)
        })))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.state
            .as_ref()
            .map_or(0, |state| (state.head - self.tail) as usize)
    }

    /// Consumes the visible bytes while retaining any acquired allocation.
    pub fn clear(&mut self) {
        if let Some(state) = &self.state {
            self.tail = state.head;
        }
    }

    /// Mutably borrows the visible suffix, copying only when another owner shares it.
    pub fn try_as_mut(&mut self) -> Result<&mut [u8], storage::AllocError> {
        let Some(state) = &mut self.state else {
            return Ok(&mut []);
        };
        window::Edit::new(state, &mut self.tail).try_as_mut()
    }
}

impl<const MAX_CAPACITY: usize> Consume for Snapshot<MAX_CAPACITY> {
    fn remaining(&self) -> usize {
        self.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        consume::Error::check(amount, self.len())?;
        self.tail += amount as u32;
        Ok(())
    }

    fn consume_max(&mut self, max: usize) -> usize {
        let amount = max.min(self.len());
        self.tail += amount as u32;
        amount
    }
}

impl<const MAX_CAPACITY: usize> AsRef<[u8]> for Snapshot<MAX_CAPACITY> {
    fn as_ref(&self) -> &[u8] {
        self.state.as_ref().map_or(&[], |state| {
            // SAFETY: construction, checked consumption, and publication keep tail
            // within head; edits preserve this same initialized suffix.
            unsafe { state.as_slice().get_unchecked(self.tail as usize..) }
        })
    }
}

impl<const MAX_CAPACITY: usize> Sink for Snapshot<MAX_CAPACITY> {
    type Error = storage::AllocError;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes.is_empty() {
            return Ok(());
        }
        let state = match &mut self.state {
            Some(state) => state,
            slot @ None => {
                let capacity = num::NonZero::<usize>::MIN.saturating_add(bytes.len() - 1);
                *slot = Some(window::Fresh::try_copy(bytes, capacity)?);
                return Ok(());
            }
        };
        window::Edit::new(state, &mut self.tail).try_extend(bytes)
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        let source = owners::Measured::try_slices(slices, MAX_CAPACITY - self.len())
            .map_err(storage::AllocError::Capacity)?;
        match &mut self.state {
            Some(state) => window::Edit::new(state, &mut self.tail).try_slices(source),
            slot @ None => {
                *slot = window::Fresh::try_slices(source)?;
                Ok(())
            }
        }
    }
}

const _: () = assert!(size_of::<Snapshot<1>>() == 3 * size_of::<usize>());

// SAFETY: Snapshot exposes its retained initialized suffix; appends and consumption
// require its exclusive borrow and cannot mutate bytes during a source loan.
unsafe impl<const MAX_CAPACITY: usize> buffer::raw::Source for Snapshot<MAX_CAPACITY> {}

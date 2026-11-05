use std::num;

use crate::buffer::{self, write, write::consume::Error};

/// Exact-length staged write shortfall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Gap(num::NonZero<u32>, u32);

/// A staged span whose publication policy is selected in its type.
#[must_use = "commit the initialized span to publish bytes"]
pub struct Writer<'writer, 'target, const EXACT: bool> {
    spare: &'writer mut write::Spare<'target>,
    limit: u32,
    len: u32,
}

impl Gap {
    pub(in crate::buffer) fn check(expected: u32, actual: u32) -> Result<(), Self> {
        if let Some(expected) = num::NonZero::new(expected)
            && actual < expected.get()
        {
            return Err(Self(expected, actual));
        }
        Ok(())
    }

    pub const fn into_parts(self) -> (num::NonZero<u32>, u32) {
        (self.0, self.1)
    }
}

impl<'writer, 'target, const EXACT: bool> Writer<'writer, 'target, EXACT> {
    pub(in crate::buffer::write) fn new(spare: &'writer mut write::Spare<'target>) -> Self {
        let limit = buffer::raw::Space::remaining(spare) as u32;
        Self {
            spare,
            limit,
            len: 0,
        }
    }

    pub(in crate::buffer) fn try_new(
        spare: &'writer mut write::Spare<'target>,
        len: usize,
    ) -> Result<Self, Error> {
        Error::check(len, buffer::raw::Space::remaining(spare))?;
        Ok(Self {
            spare,
            limit: len as u32,
            len: 0,
        })
    }

    pub(in crate::buffer) fn suffix(&mut self) -> (*mut u8, usize) {
        let start = self.spare.len() + self.len as usize;
        (self.spare.pointer.as_ptr().cast(), start)
    }

    /// # Safety
    /// Exactly `copied` admitted bytes were initialized in this writer's suffix.
    pub(in crate::buffer) unsafe fn extend(&mut self, copied: usize) {
        self.len += copied as u32;
    }

    fn publish(self) {
        self.spare.len += self.len;
    }

    /// Initializes one complete source after capacity admission. A shortfall
    /// leaves the destination unchanged and does not call the initializer.
    pub fn try_init<I: buffer::raw::Initializer + ?Sized>(
        &mut self,
        source: &I,
    ) -> Result<(), Error> {
        let len = source.len();
        Error::check(len, buffer::raw::Space::remaining(self))?;
        let (target, start) = self.suffix();
        // SAFETY: admission fits the source's stable length in this exclusive
        // suffix; its retained sources are disjoint from the writer's storage.
        unsafe { source.initialize(target.add(start).cast()) };
        // SAFETY: Initializer initialized exactly the admitted prefix before publication.
        unsafe { self.extend(len) };
        Ok(())
    }
}

impl<'writer, 'target> Writer<'writer, 'target, false> {
    pub fn commit(self) {
        self.publish();
    }
}

impl<'writer, 'target> Writer<'writer, 'target, true> {
    pub fn try_commit(self) -> Result<(), Gap> {
        Gap::check(self.limit, self.len)?;
        self.publish();
        Ok(())
    }
}

impl AsMut<[u8]> for Writer<'_, '_, true> {
    fn as_mut(&mut self) -> &mut [u8] {
        // SAFETY: Writer advances len only after initializing each byte and
        // retains exclusive access to the parent writer until publication.
        unsafe {
            buffer::raw::range_mut(
                self.spare.pointer.as_ptr().cast(),
                self.spare.len(),
                self.len as usize,
            )
        }
    }
}

impl<const EXACT: bool> write::Sink for Writer<'_, '_, EXACT> {
    type Error = Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        write::Staging::try_new(self, slices).map(buffer::raw::Copy::copy)
    }
}

impl<const EXACT: bool> buffer::raw::Space for Writer<'_, '_, EXACT> {
    fn remaining(&self) -> usize {
        (self.limit - self.len) as usize
    }
}

const _: () = assert!(size_of::<Gap>() == size_of::<u64>());
const _: () = assert!(size_of::<Option<Gap>>() == size_of::<u64>());
const _: () = assert!(size_of::<Result<(), Gap>>() == size_of::<u64>());
const _: () = assert!(size_of::<Writer<'static, 'static, false>>() == size_of::<usize>() * 2);
const _: () = assert!(size_of::<Writer<'static, 'static, true>>() == size_of::<usize>() * 2);

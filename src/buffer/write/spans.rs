use std::num;

use crate::{
    buffer,
    buffer::{segments::scatter, write, write::consume::Error},
};

/// Exact-length staged write shortfall.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Gap(num::NonZero<u32>, u32);

/// A staged span whose publication policy is selected in its type.
#[must_use = "commit the initialized span to publish bytes"]
#[repr(transparent)]
pub struct Writer<'writer, 'target, const EXACT: bool> {
    pub(in crate::buffer) cursor: write::Cursor<'writer, 'target>,
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

impl<const EXACT: bool> Writer<'_, '_, EXACT> {
    /// Initializes one complete source after capacity admission. A shortfall
    /// leaves the destination unchanged and does not call the initializer.
    pub fn try_init<I: buffer::raw::Initializer + ?Sized>(
        &mut self,
        source: &I,
    ) -> Result<(), Error> {
        let len = source.len();
        Error::check(len, buffer::raw::Space::remaining(&self.cursor))?;
        let (target, start) = self.cursor.suffix();
        // SAFETY: admission fits the source's stable length in this exclusive
        // suffix; its retained sources are disjoint from the writer's storage.
        unsafe { source.initialize(target.add(start).cast()) };
        // SAFETY: Initializer initialized exactly the admitted prefix before publication.
        unsafe { self.cursor.extend(len) };
        Ok(())
    }
}

impl<'writer, 'target> Writer<'writer, 'target, false> {
    pub fn commit(self) {
        self.cursor.commit();
    }
}

impl<'writer, 'target> Writer<'writer, 'target, true> {
    pub fn try_commit(self) -> Result<(), Gap> {
        Gap::check(self.cursor.limit(), self.cursor.len())?;
        self.cursor.commit();
        Ok(())
    }
}

impl AsMut<[u8]> for Writer<'_, '_, true> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.cursor.as_mut()
    }
}

impl<const EXACT: bool> write::Sink for Writer<'_, '_, EXACT> {
    type Error = Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        write::Staging::try_new(&mut self.cursor, slices).map(buffer::raw::Copy::copy)
    }
}

const _: () = assert!(size_of::<Gap>() == size_of::<u64>());
const _: () = assert!(size_of::<Option<Gap>>() == size_of::<u64>());
const _: () = assert!(size_of::<Result<(), Gap>>() == size_of::<u64>());
const _: () = assert!(size_of::<Writer<'static, 'static, false>>() == size_of::<usize>() * 2);
const _: () = assert!(size_of::<Writer<'static, 'static, true>>() == size_of::<usize>() * 2);

/// An initialized body with a fixed header and an admitted, uninitialized tail.
#[must_use = "call commit with the complete tail to publish the write"]
#[repr(transparent)]
pub struct Tail<'writer, 'target, const HEAD: usize, const N: usize> {
    cursor: write::Cursor<'writer, 'target>,
}

impl<'writer, 'target, const HEAD: usize, const N: usize> Tail<'writer, 'target, HEAD, N> {
    pub fn try_new<const S: usize>(
        writer: &'writer mut write::Spare<'target>,
        header: &[u8; HEAD],
        body: [&[u8]; S],
    ) -> Result<Self, write::Error> {
        let mut length = HEAD;
        for source in &body {
            length = write::Overflow::add(length, source.len()).map_err(write::Error::Overflow)?;
        }
        let length = write::Overflow::add(length, N).map_err(write::Error::Overflow)?;
        let mut cursor = write::Cursor::try_new(writer, length).map_err(write::Error::Capacity)?;
        buffer::raw::Copy::copy(Init {
            cursor: &mut cursor,
            sources: (header.as_slice(), body),
            len: length - N,
        });
        Ok(Self { cursor })
    }

    pub fn try_scatter<'cursor, 'data, C: buffer::raw::Scatter<'data>>(
        writer: &'writer mut write::Spare<'target>,
        header: &[u8; HEAD],
        body: scatter::Take<'cursor, 'data, C>,
        suffix: &[u8],
    ) -> Result<Self, write::Error> {
        let length = [HEAD, body.len(), suffix.len(), N]
            .into_iter()
            .try_fold(0, write::Overflow::add)
            .map_err(write::Error::Overflow)?;
        let mut cursor = write::Cursor::try_new(writer, length).map_err(write::Error::Capacity)?;
        buffer::raw::Copy::copy(Init {
            cursor: &mut cursor,
            sources: header.as_slice(),
            len: HEAD,
        });
        for part in body {
            buffer::raw::Copy::copy(Init {
                cursor: &mut cursor,
                sources: part.bytes(),
                len: part.len().get(),
            });
        }
        buffer::raw::Copy::copy(Init {
            cursor: &mut cursor,
            sources: suffix,
            len: suffix.len(),
        });
        Ok(Self { cursor })
    }

    pub fn split(&mut self) -> (&[u8; HEAD], &mut [u8]) {
        // SAFETY: both constructors initialize HEAD bytes in this exclusive
        // cursor, and no operation shortens that initialized prefix.
        let (header, body) = unsafe { self.cursor.as_mut().split_at_mut_unchecked(HEAD) };
        // SAFETY: the split retains exactly HEAD initialized bytes, disjoint from body.
        unsafe { (&*header.as_ptr().cast(), body) }
    }

    pub fn commit(mut self, tail: &[u8; N]) {
        buffer::raw::Copy::copy(Init {
            cursor: &mut self.cursor,
            sources: tail.as_slice(),
            len: N,
        });
        self.cursor.commit();
    }
}

/// Sources bound to this cursor after total admission by a Tail constructor.
struct Init<'cursor, 'writer, 'target, I> {
    cursor: &'cursor mut write::Cursor<'writer, 'target>,
    sources: I,
    len: usize,
}

// SAFETY: only this module constructs Init: the constructors retain their
// admitted sources or exact Take traversal, and commit retains N spare
// bytes for its fixed tail. Every stage keeps the original exclusive cursor.
unsafe impl<I: buffer::raw::SliceExt> buffer::raw::Copy for Init<'_, '_, '_, I> {
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        let (target, start) = self.cursor.suffix();
        (&self.sources, target, start)
    }

    unsafe fn publish(self) {
        // SAFETY: Copy initialized this admitted suffix while retaining the same cursor.
        unsafe { self.cursor.extend(self.len) };
    }
}

const _: () = {
    assert!(size_of::<Tail<'static, 'static, 5, 16>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Tail<'static, 'static, 5, 16>>>() == 2 * size_of::<usize>());
};

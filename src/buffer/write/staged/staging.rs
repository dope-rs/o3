use crate::{
    base::num::bounded,
    buffer,
    buffer::{write, write::consume::Error},
};

/// One admitted multi-source copy into an exclusive staged suffix.
#[must_use]
pub(in crate::buffer) struct Staging<'a, 'source, W, const N: usize, E = usize> {
    slices: [&'source [u8]; N],
    writer: &'a mut W,
    len: E,
}

impl<'a, 'source, W: crate::buffer::raw::Space, const N: usize> Staging<'a, 'source, W, N> {
    pub(in crate::buffer) fn try_new(
        writer: &'a mut W,
        slices: [&'source [u8]; N],
    ) -> Result<Self, Error> {
        let len = Error::checked_end(0, writer.remaining(), &slices)?;
        Ok(Self {
            writer,
            slices,
            len,
        })
    }
}

impl<'a, 'source, 'target> Staging<'a, 'source, write::Spare<'target>, 1> {
    pub(in crate::buffer) fn prefix(
        writer: &'a mut write::Spare<'target>,
        src: &'source [u8],
    ) -> (Self, &'source [u8]) {
        let len = src.len().min(crate::buffer::raw::Space::remaining(writer));
        let (head, tail) = src.split_at(len);
        (
            Self {
                writer,
                slices: [head],
                len,
            },
            tail,
        )
    }

    pub(in crate::buffer) fn len(&self) -> usize {
        self.len
    }
}

impl<'a, 'source, 'target, const N: usize, const S: usize>
    Staging<'a, 'source, write::Array<'target, N>, S, bounded::Usize<N>>
{
    pub(in crate::buffer) fn try_array(
        writer: &'a mut write::Array<'target, N>,
        slices: [&'source [u8]; S],
    ) -> Result<Self, bounded::Error> {
        let mut end = writer.written;
        for source in &slices {
            end = end.try_add(source.len())?;
        }
        Ok(Self {
            writer,
            slices,
            len: end,
        })
    }
}

// SAFETY: Staging retains this cursor's admitted suffix and every source.
unsafe impl<const N: usize> buffer::raw::Copy for Staging<'_, '_, write::Cursor<'_, '_>, N> {
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        let (target, start) = self.writer.suffix();
        (self.slices, target, start)
    }

    unsafe fn publish(self) {
        // SAFETY: copying every admitted source initialized this exact suffix.
        unsafe { self.writer.extend(self.len) };
    }
}

// SAFETY: Staging retains the block's exclusive suffix under complete source admission.
unsafe impl<W: buffer::raw::Exclusive, const N: usize> buffer::raw::Copy for Staging<'_, '_, W, N> {
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        let pointer = self.writer.as_mut_ptr();
        let len = self.writer.len_mut();
        (self.slices, pointer.cast(), *len as usize)
    }

    unsafe fn publish(self) {
        *self.writer.len_mut() += self.len as u32;
    }
}

// SAFETY: Staging retains the exact output slice and its admitted disjoint sources.
unsafe impl<const N: usize> buffer::raw::Copy for Staging<'_, '_, write::Slice<'_>, N> {
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (self.slices, self.writer.current, 0)
    }

    unsafe fn publish(self) {
        // SAFETY: admission retained this complete suffix before the copy.
        self.writer.current = unsafe { self.writer.current.add(self.len) };
        self.writer.remaining -= self.len;
    }
}

// SAFETY: the bounded end admits every source in this exact exclusive array suffix.
unsafe impl<const N: usize, const S: usize> buffer::raw::Copy
    for Staging<'_, '_, write::Array<'_, N>, S, bounded::Usize<N>>
{
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (
            self.slices,
            self.writer.out.as_mut_ptr(),
            self.writer.written.get(),
        )
    }

    unsafe fn publish(self) {
        self.writer.written = self.len;
    }
}

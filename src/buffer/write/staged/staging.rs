use crate::buffer::{self, write, write::consume::Error};

/// One admitted multi-source copy into an exclusive staged suffix.
#[must_use]
pub(in crate::buffer) struct Staging<'a, 'source, W, const N: usize> {
    slices: [&'source [u8]; N],
    writer: &'a mut W,
    len: usize,
}

impl<'a, 'source, W: crate::buffer::raw::Exclusive, const N: usize> Staging<'a, 'source, W, N> {
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
        let len = src.len().min(writer.remaining());
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

impl<W: buffer::raw::Exclusive, const N: usize> Staging<'_, '_, W, N> {
    pub(in crate::buffer) fn copy(self) {
        use buffer::raw::SliceExt as _;

        let pointer = self.writer.as_mut_ptr();
        let len = self.writer.len_mut();
        let start = *len;
        // SAFETY: admission retains these sources and their exclusive suffix;
        // Exclusive keeps the length word disjoint from the written range.
        unsafe { self.slices.copy_to(pointer.add(start as usize).cast()) };
        *len = start + self.len as u32;
    }
}

use crate::{buffer, buffer::write::consume::Error};

#[must_use = "measured input must be consumed"]
pub(in crate::buffer) struct Measured<I> {
    input: I,
    len: usize,
}

pub(in crate::buffer) trait Input: Copy + buffer::raw::SliceExt {
    fn try_measure(self) -> Result<Measured<Self>, Error>;
}

impl<I> Measured<I> {
    pub(in crate::buffer) const fn len(&self) -> usize {
        self.len
    }

    pub(in crate::buffer) fn into_input(self) -> I {
        self.input
    }

    pub(in crate::buffer) const fn input(&self) -> &I {
        &self.input
    }
}

impl<'a> Measured<&'a [u8]> {
    pub(in crate::buffer) fn slice(input: &'a [u8]) -> Self {
        Self {
            len: input.len(),
            input,
        }
    }
}

impl<'a, const N: usize> Measured<[&'a [u8]; N]> {
    pub(in crate::buffer) fn try_slices(
        input: [&'a [u8]; N],
        capacity: usize,
    ) -> Result<Self, Error> {
        let len = Error::checked_end(0, capacity, &input)?;
        Ok(Self { input, len })
    }
}

impl Input for &[u8] {
    fn try_measure(self) -> Result<Measured<Self>, Error> {
        Ok(Measured::slice(self))
    }
}

impl<const N: usize> Input for [&[u8]; N] {
    fn try_measure(self) -> Result<Measured<Self>, Error> {
        Measured::try_slices(self, usize::MAX)
    }
}

impl<const P: usize, const N: usize> Input for ([u8; P], [&[u8]; N]) {
    fn try_measure(self) -> Result<Measured<Self>, Error> {
        let len = Error::checked_end(P, usize::MAX, &self.1)?;
        Ok(Measured { input: self, len })
    }
}

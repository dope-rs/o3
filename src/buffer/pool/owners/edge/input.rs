use crate::buffer::write::consume::Error;

#[must_use = "measured input must be consumed"]
pub(in crate::buffer) struct Measured<I> {
    input: I,
    len: usize,
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

impl<'a, const P: usize, const N: usize> Measured<([u8; P], [&'a [u8]; N])> {
    pub(in crate::buffer) fn try_prefixed(input: ([u8; P], [&'a [u8]; N])) -> Result<Self, Error> {
        let len = Error::checked_end(P, usize::MAX, &input.1)?;
        Ok(Self { input, len })
    }
}

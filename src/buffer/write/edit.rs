use crate::buffer::{
    write,
    write::{consume, spans},
};

/// Exclusive editable prefix and staged writes for one buffer owner.
#[must_use = "commit the staged suffix after editing the prefix"]
#[repr(C)]
pub struct Edit<'a, const N: usize> {
    prefix: &'a mut [u8; N],
    spare: write::Spare<'a>,
}

/// Rejected initialized-prefix edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub width: usize,
    pub len: usize,
}

impl Error {
    pub(in crate::buffer) const fn new(offset: usize, width: usize, len: usize) -> Self {
        Self { offset, width, len }
    }
}

impl<'a, const N: usize> Edit<'a, N> {
    /// Stages a new fixed prefix without publishing its owner.
    /// Unlike editing an existing prefix, `len` includes earlier staged bytes and
    /// this new prefix; `commit` publishes the complete staged write.
    pub fn try_new(mut spare: write::Spare<'a>, prefix: &[u8; N]) -> Result<Self, consume::Error> {
        let start = spare.len();
        write::Sink::write_slice(&mut spare, prefix)?;
        // SAFETY: admission initialized this exact array. All appends start
        // after it, and the retained spare keeps the backing exclusively owned.
        let prefix = unsafe { &mut *spare.pointer.as_ptr().add(start).cast::<[u8; N]>() };
        Ok(Self::new(prefix, spare))
    }

    pub(in crate::buffer) fn new(prefix: &'a mut [u8; N], spare: write::Spare<'a>) -> Self {
        Self { prefix, spare }
    }

    pub fn bounded(&mut self) -> spans::Writer<'_, 'a, false> {
        self.spare.bounded()
    }

    /// Stages another suffix while retaining this edit's disjoint fixed prefix.
    pub fn spare(&mut self) -> write::Spare<'_> {
        write::SpareExt::spare(&mut self.spare)
    }

    pub fn prefix(&mut self) -> &mut [u8; N] {
        self.prefix
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.spare.len()
    }

    pub fn commit(self) {
        self.spare.commit();
    }
}

impl<const N: usize> write::Sink for Edit<'_, N> {
    type Error = consume::Error;

    fn write_slices<const M: usize>(&mut self, slices: [&[u8]; M]) -> Result<(), Self::Error> {
        self.spare.write_slices(slices)
    }
}

const _: () = assert!(size_of::<Edit<'static, 1>>() == 4 * size_of::<usize>());
const _: () = assert!(size_of::<Error>() == 3 * size_of::<usize>());

use crate::{
    base::num::bounded,
    buffer::{self, write},
};

/// A fixed destination retaining its initialized length as a bounded value.
pub struct Array<'a, const N: usize> {
    out: &'a mut [u8; N],
    written: bounded::Usize<N>,
}

impl<'a, const N: usize> Array<'a, N> {
    pub fn new(out: &'a mut [u8; N]) -> Self {
        Self {
            out,
            written: bounded::Usize::ZERO,
        }
    }

    pub fn finish(self) -> bounded::Usize<N> {
        self.written
    }
}

impl<const N: usize> write::Sink for Array<'_, N> {
    type Error = bounded::Error;

    fn write_slices<const S: usize>(&mut self, sources: [&[u8]; S]) -> Result<(), Self::Error> {
        use buffer::raw::SliceExt as _;

        let mut end = self.written;
        for source in &sources {
            end = end.try_add(source.len())?;
        }
        // SAFETY: the bounded end admits every source in this exclusive array suffix.
        unsafe { sources.copy_to(self.out.as_mut_ptr().add(self.written.get())) };
        self.written = end;
        Ok(())
    }
}

const _: () = {
    assert!(size_of::<Array<'static, 16_384>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Array<'static, 16_384>>>() == size_of::<Array<'static, 16_384>>());
};

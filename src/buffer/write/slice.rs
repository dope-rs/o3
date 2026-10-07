use std::{marker, num::NonZero, ptr};

use crate::buffer::{self, write, write::consume};

/// A checked cursor retaining its complete initialized output slice.
pub struct Slice<'a> {
    base: ptr::NonNull<u8>,
    pub(super) current: *mut u8,
    pub(super) remaining: usize,
    borrow: marker::PhantomData<&'a mut [u8]>,
}

impl<'a> Slice<'a> {
    pub fn new(out: &'a mut [u8]) -> Self {
        let len = out.len();
        let base = ptr::NonNull::from(out).cast();
        Self {
            base,
            current: base.as_ptr(),
            remaining: len,
            borrow: marker::PhantomData,
        }
    }

    pub fn finish(self) -> usize {
        self.current.addr() - self.base.addr().get()
    }

    pub const fn remaining(&self) -> usize {
        self.remaining
    }

    /// Returns exactly the written prefix under the original exclusive borrow.
    pub fn into_mut(self) -> &'a mut [u8] {
        // SAFETY: admission keeps current at or before the original output's end;
        // consuming this cursor transfers exactly its exclusive initialized prefix.
        unsafe { buffer::raw::range_mut(self.base.as_ptr(), 0, self.finish()) }
    }

    /// Succeeds only after the complete original output slice was written.
    pub fn try_complete(self) -> Result<(), consume::Error> {
        let Some(left) = NonZero::new(self.remaining) else {
            return Ok(());
        };
        let written = self.finish();
        Err(consume::Error::new(left.saturating_add(written), written))
    }
}

impl write::Sink for Slice<'_> {
    type Error = consume::Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        write::Staging::try_new(self, slices).map(buffer::raw::Copy::copy)
    }
}

impl buffer::raw::Space for Slice<'_> {
    fn remaining(&self) -> usize {
        self.remaining()
    }
}

const _: () = {
    assert!(size_of::<Slice<'static>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Slice<'static>>>() == size_of::<Slice<'static>>());
};

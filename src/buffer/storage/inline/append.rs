use std::mem::size_of;

/// Exclusive reservation that publishes its initialized region on commit.
#[must_use = "commit the reservation to publish its region"]
#[repr(C)]
pub struct Append<'a, L = u8> {
    output: &'a mut [u8],
    len: &'a mut L,
}

impl<'a, L: crate::buffer::raw::Length> Append<'a, L> {
    pub(in crate::buffer::storage::inline) fn new(output: &'a mut [u8], len: &'a mut L) -> Self {
        Self { output, len }
    }

    pub fn write<R>(self, write: impl FnOnce(&mut [u8]) -> R) -> R {
        let value = write(self.output);
        self.commit();
        value
    }

    pub fn try_write<R, E>(self, write: impl FnOnce(&mut [u8]) -> Result<R, E>) -> Result<R, E> {
        let value = write(self.output)?;
        self.commit();
        Ok(value)
    }

    /// Publishes the complete reserved region and consumes its exclusive borrow.
    pub fn commit(self) {
        // SAFETY: admission bounds this reserved suffix by the original Bytes
        // capacity, whose construction proves that capacity fits L.
        *self.len = unsafe { L::add(*self.len, self.output.len()) };
    }
}

impl<L> AsMut<[u8]> for Append<'_, L> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.output
    }
}

const _: () = assert!(size_of::<Append<'static>>() == 3 * size_of::<usize>());
const _: () = assert!(size_of::<Option<Append<'static>>>() == size_of::<Append<'static>>());

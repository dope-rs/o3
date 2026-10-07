use crate::buffer::{write, write::consume::Error};

pub(in crate::buffer) struct Cursor<'writer, 'target> {
    pub(in crate::buffer::write) spare: &'writer mut write::Spare<'target>,
    limit: u32,
    pub(in crate::buffer::write) len: u32,
}

impl<'writer, 'target> Cursor<'writer, 'target> {
    pub(in crate::buffer) fn new(spare: &'writer mut write::Spare<'target>) -> Self {
        let limit = crate::buffer::raw::Space::remaining(spare) as u32;
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
        Error::check(len, crate::buffer::raw::Space::remaining(spare))?;
        Ok(Self {
            spare,
            limit: len as u32,
            len: 0,
        })
    }

    pub(in crate::buffer) fn len(&self) -> u32 {
        self.len
    }

    pub(in crate::buffer) fn limit(&self) -> u32 {
        self.limit
    }

    pub(in crate::buffer) fn suffix(&mut self) -> (*mut u8, usize) {
        let start = self.spare.len() + self.len as usize;
        (self.spare.pointer.as_ptr().cast(), start)
    }

    /// # Safety
    /// Exactly `copied` admitted bytes were initialized in this cursor's suffix.
    pub(in crate::buffer) unsafe fn extend(&mut self, copied: usize) {
        self.len += copied as u32;
    }

    pub(in crate::buffer) fn as_mut(&mut self) -> &mut [u8] {
        // SAFETY: Cursor advances len only after initializing each byte and
        // retains exclusive access to the parent writer until publication.
        unsafe {
            crate::buffer::raw::range_mut(
                self.spare.pointer.as_ptr().cast(),
                self.spare.len(),
                self.len as usize,
            )
        }
    }

    pub(in crate::buffer) fn commit(self) {
        self.spare.len += self.len;
    }
}

impl crate::buffer::raw::Space for Cursor<'_, '_> {
    fn remaining(&self) -> usize {
        (self.limit - self.len) as usize
    }
}

use crate::{
    buffer,
    buffer::{bytes, pool},
};

/// A cleared fixed-width cursor with a compile-time remaining write bound.
pub struct Fill<'a, const CAP: usize, const LEFT: usize = CAP> {
    target: &'a mut pool::Cursor<pool::Width<CAP>>,
}

struct Write<'a, 'source, const CAP: usize, const MAX: usize> {
    target: &'a mut pool::Cursor<pool::Width<CAP>>,
    source: bytes::Prefix<'source, MAX>,
    pointer: *mut u8,
}

impl<'a, const CAP: usize> Fill<'a, CAP> {
    pub fn new(target: &'a mut pool::Cursor<pool::Width<CAP>>) -> Self {
        target.clear();
        Self { target }
    }
}

impl<'a, const CAP: usize, const LEFT: usize> Fill<'a, CAP, LEFT> {
    pub fn write<const MAX: usize, const NEXT: usize>(
        mut self,
        source: bytes::Prefix<'_, MAX>,
    ) -> (Fill<'a, CAP, NEXT>, &'a mut [u8]) {
        const { assert!(NEXT == LEFT - MAX) }
        let write = self.prepare(source);
        let pointer = write.pointer;
        let len = write.source.as_slice().len();
        buffer::raw::Copy::copy(write);
        // SAFETY: publication initialized only this disjoint suffix under the retained loan.
        let copied = unsafe { buffer::raw::range_mut(pointer, 0, len) };
        (
            Fill {
                target: self.target,
            },
            copied,
        )
    }

    pub fn finish(mut self, source: bytes::Prefix<'_, LEFT>) {
        buffer::raw::Copy::copy(self.prepare(source));
    }

    fn prepare<'source, const MAX: usize>(
        &mut self,
        source: bytes::Prefix<'source, MAX>,
    ) -> Write<'_, 'source, CAP, MAX> {
        const { assert!(MAX <= LEFT) }
        let cursor = &mut *self.target;
        // SAFETY: Fill retains the exact suffix admitted by this static transition.
        let pointer = unsafe { cursor.lease.entry().data().add(cursor.lease.len()) };
        Write {
            target: cursor,
            source,
            pointer,
        }
    }
}

// SAFETY: Fill's static transition admits only the disjoint suffix after earlier writes.
unsafe impl<const CAP: usize, const MAX: usize> buffer::raw::Copy for Write<'_, '_, CAP, MAX> {
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        ([self.source.as_slice()], self.pointer, 0)
    }

    unsafe fn publish(self) {
        self.target.lease.len += self.source.as_slice().len() as u32;
    }
}

const _: () = assert!(size_of::<Fill<'_, 326>>() == size_of::<usize>());

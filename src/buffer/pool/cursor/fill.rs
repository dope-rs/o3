use crate::{
    buffer,
    buffer::{bytes, pool, pool::owners},
};

/// A cleared fixed-width cursor with a compile-time remaining write bound.
pub struct Fill<'a, const CAP: usize, const LEFT: usize = CAP> {
    target: &'a mut pool::Cursor<pool::Width<CAP>>,
}

impl<'a, const CAP: usize> Fill<'a, CAP> {
    pub fn new(target: &'a mut pool::Cursor<pool::Width<CAP>>) -> Self {
        target.clear();
        Self { target }
    }
}

impl<'a, const CAP: usize, const LEFT: usize> Fill<'a, CAP, LEFT> {
    pub fn write<const MAX: usize, const NEXT: usize>(
        self,
        source: bytes::Prefix<'_, MAX>,
    ) -> (Fill<'a, CAP, NEXT>, &'a mut [u8]) {
        const { assert!(MAX <= LEFT && NEXT == LEFT - MAX) }
        let target = &mut self.target.lease;
        // SAFETY: Fill retains the exact suffix admitted by this static transition.
        let pointer = unsafe { target.entry().data().add(target.len()) };
        let input = owners::Measured::slice(source.as_slice());
        let len = input.len();
        // The static transition admits this suffix without touching earlier returned slices.
        pool::Append { target, input }.commit();
        // SAFETY: publication initialized only this disjoint suffix under the retained loan.
        let copied = unsafe { buffer::raw::range_mut(pointer, 0, len) };
        (
            Fill {
                target: self.target,
            },
            copied,
        )
    }

    pub fn finish(self, source: bytes::Prefix<'_, LEFT>) {
        let _ = self.write::<LEFT, 0>(source);
    }
}

const _: () = assert!(size_of::<Fill<'_, 326>>() == size_of::<usize>());

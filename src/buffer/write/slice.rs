use std::{borrow::BorrowMut, marker, mem::MaybeUninit, ptr};

use crate::buffer::{self, write, write::consume};

/// A checked writer exposing only its initialized prefix.
pub struct Slice<'a, L = usize> {
    base: ptr::NonNull<u8>,
    len: L,
    capacity: usize,
    borrow: marker::PhantomData<&'a mut [MaybeUninit<u8>]>,
}

impl<'a> Slice<'a> {
    pub fn new(out: &'a mut [u8]) -> Self {
        let capacity = out.len();
        Self::from_parts(ptr::NonNull::from(out).cast(), 0, capacity)
    }

    pub fn uninit(out: &'a mut [MaybeUninit<u8>]) -> Self {
        let capacity = out.len();
        Self::from_parts(ptr::NonNull::from(out).cast(), 0, capacity)
    }
}

impl<'a, L> Slice<'a, L> {
    fn from_parts(base: ptr::NonNull<u8>, len: L, capacity: usize) -> Self {
        Self {
            base,
            len,
            capacity,
            borrow: marker::PhantomData,
        }
    }
}

impl<'a, L: BorrowMut<usize>> Slice<'a, L> {
    pub fn len(&self) -> usize {
        *self.len.borrow()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn finish(self) -> usize {
        self.len()
    }

    pub fn remaining(&self) -> usize {
        self.capacity - self.len()
    }

    /// Lends this writer's actual progress without lending its identity.
    pub fn reborrow(&mut self) -> Slice<'_, &mut usize> {
        Slice::from_parts(self.base, self.len.borrow_mut(), self.capacity)
    }

    /// Returns exactly the written prefix under the original exclusive borrow.
    pub fn into_mut(self) -> &'a mut [u8] {
        // SAFETY: only successful writes extend this exact initialized prefix;
        // a lent count retains its parent's exclusive borrow for the returned slice.
        unsafe { buffer::raw::range_mut(self.base.as_ptr(), 0, self.finish()) }
    }

    /// Succeeds only after the complete original output slice was written.
    pub fn try_complete(self) -> Result<(), consume::Error> {
        consume::Error::check(self.capacity, self.len())
    }

    /// Stages adjacent head and body regions without advancing this writer.
    pub fn try_split(
        &mut self,
        head: usize,
        body: usize,
    ) -> Result<Split<'_, 'a, L>, consume::Error> {
        let room = self.remaining();
        consume::Error::check(head, room)?;
        consume::Error::check(body, room - head)?;
        Ok(Split {
            parent: self,
            head,
            capacity: head + body,
            head_len: 0,
            body_len: 0,
        })
    }
}

impl<L: BorrowMut<usize>> write::Sink for Slice<'_, L> {
    type Error = consume::Error;

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        use buffer::raw::SliceExt as _;

        let start = self.len();
        let len = consume::Error::checked_end(0, self.capacity - start, &slices)?;
        // SAFETY: admission retains this complete suffix and its disjoint sources.
        unsafe {
            slices.copy_to(self.base.as_ptr().add(start));
        }
        *self.len.borrow_mut() = start + len;
        Ok(())
    }
}

/// Unpublished adjacent regions retained by one parent writer.
pub struct Split<'writer, 'target, L = usize> {
    parent: &'writer mut Slice<'target, L>,
    head: usize,
    capacity: usize,
    head_len: usize,
    body_len: usize,
}

impl<L: BorrowMut<usize>> Split<'_, '_, L> {
    pub fn head(&mut self) -> Slice<'_, &mut usize> {
        // SAFETY: admission retained this parent's entire unpublished suffix.
        let base = unsafe { self.parent.base.add(self.parent.len()) };
        Slice::from_parts(base, &mut self.head_len, self.head)
    }

    pub fn body(&mut self) -> Slice<'_, &mut usize> {
        // SAFETY: the admitted head separates this body's exact disjoint region.
        let base = unsafe { self.parent.base.add(self.parent.len() + self.head) };
        Slice::from_parts(base, &mut self.body_len, self.capacity - self.head)
    }

    /// Publishes the contiguous prefix only after the entire head is initialized.
    pub fn try_commit(self) -> Result<(), consume::Error> {
        consume::Error::check(self.head, self.head_len)?;
        *self.parent.len.borrow_mut() += self.head + self.body_len;
        Ok(())
    }
}

const _: () = {
    assert!(size_of::<Slice<'static>>() == 3 * size_of::<usize>());
    assert!(size_of::<Option<Slice<'static>>>() == size_of::<Slice<'static>>());
    assert!(size_of::<Slice<'static, &'static mut usize>>() == 3 * size_of::<usize>());
    assert!(size_of::<Split<'static, 'static>>() == 5 * size_of::<usize>());
};

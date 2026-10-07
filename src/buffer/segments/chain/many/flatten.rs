use std::{num::NonZero, ops::ControlFlow};

use crate::{
    buffer::{
        self,
        segments::{Many, Nonempty},
        storage,
    },
    mem::Rejected,
};

/// One allocation and an owned segment position for bounded copying.
#[must_use = "step or release the retained copy"]
pub struct Flatten<T: buffer::raw::Segment> {
    pub(in crate::buffer) source: Nonempty<T>,
    pub(in crate::buffer) offset: usize,
    pub(in crate::buffer) output: storage::Owned,
}

impl<T: buffer::raw::Segment> Flatten<T> {
    /// Reserves the complete output before consuming any source owner.
    pub fn try_new(source: Many<T>) -> Result<Self, Rejected<Many<T>, storage::AllocError>> {
        let output = match storage::Owned::try_with(source.len().get()) {
            Ok(output) => output,
            Err(error) => return Err(Rejected::new(source, error)),
        };
        Ok(Self {
            source: Nonempty::Many(source),
            offset: 0,
            output,
        })
    }

    /// Copies at most `budget` bytes without allocating or rescanning prior owners.
    /// Only completion exposes the fully initialized output.
    pub fn step(mut self, mut budget: NonZero<usize>) -> ControlFlow<storage::Shared, Self> {
        loop {
            let copied = buffer::raw::Copy::copy((&mut self, budget));
            let left = budget.get() - copied.get();
            if self.offset < self.source.front().as_ref().len() {
                return ControlFlow::Continue(self);
            }
            self.source = match self.source {
                Nonempty::One(_) => return ControlFlow::Break(self.output.freeze()),
                Nonempty::Many(source) => source.take_front().1,
            };
            self.offset = 0;
            let Some(left) = NonZero::new(left) else {
                return ControlFlow::Continue(self);
            };
            budget = left;
        }
    }
}

// SAFETY: Flatten owns both the unchanged source suffix and its separately
// allocated output. Admission reserves the full source length; every copy
// advances source offset and initialized output by the same amount. Thus all
// remaining source bytes fit the output's spare capacity, even if allocation
// rounded that capacity up. The pair cannot substitute a different source,
// target, or offset, and its exclusive loan preserves them through publication.
// Parts stores the exact positive copied amount for publication.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Copy for (&mut Flatten<T>, NonZero<usize>) {
    type Output = NonZero<usize>;

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        let source = self.0.source.front().as_ref();
        let amount = self.1.get().min(source.len() - self.0.offset);
        // SAFETY: the strict offset leaves nonempty bytes and the budget is positive.
        self.1 = unsafe { NonZero::new_unchecked(amount) };
        // SAFETY: offset and amount retain this unchanged front's in-bounds prefix.
        let source = unsafe { source.get_unchecked(self.0.offset..self.0.offset + amount) };
        let output = &mut *self.0.output;
        let pointer = buffer::raw::Exclusive::as_mut_ptr(output);
        let start = *buffer::raw::Exclusive::len_mut(output) as usize;
        (source, pointer.cast(), start)
    }

    unsafe fn publish(self) -> NonZero<usize> {
        let amount = self.1.get();
        // SAFETY: Copy initialized this exact reserved output suffix; both
        // original positions remain unchanged until this paired publication.
        *buffer::raw::Exclusive::len_mut(&mut *self.0.output) += amount as u32;
        self.0.offset += amount;
        self.1
    }
}

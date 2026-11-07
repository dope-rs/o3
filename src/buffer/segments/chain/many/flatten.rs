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
    source: Nonempty<T>,
    offset: usize,
    output: storage::Owned,
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
            let source = self.source.front().as_ref();
            let remaining = source.len() - self.offset;
            let amount = budget.get().min(remaining);
            let prefix = self.output.len() as u32;
            // SAFETY: positive budget and strict offset select an exact nonempty range.
            // The admitted total fits u32 and reserves the remaining bytes in a disjoint
            // output; its current initialized length is this replacement's prefix.
            unsafe {
                let source = source.get_unchecked(self.offset..self.offset + amount);
                buffer::raw::Slices::new(NonZero::new_unchecked(amount as u32), [source])
                    .replace(&mut self.output, prefix);
            }
            let left = budget.get() - amount;
            if amount < remaining {
                self.offset += amount;
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

use std::num::NonZero;

use segments::queue;

use crate::{
    buffer,
    buffer::{segments, write::consume},
};

/// A validated prefix retaining its exclusive owner through inspection and commit.
#[must_use]
pub struct Drain<'a, T> {
    pub(in crate::buffer::segments) target: &'a mut T,
    pub(in crate::buffer::segments) amount: usize,
}

impl<'a, T: consume::Consume> Drain<'a, T> {
    pub fn try_new(target: &'a mut T, amount: usize) -> Result<Self, consume::Error> {
        consume::Error::check(amount, target.remaining())?;
        Ok(Self { target, amount })
    }
}

impl<T: buffer::raw::Segment> Drain<'_, queue::Fifo<T>> {
    pub fn commit(self) {
        use segments::Tail;

        let queue = self.target;
        queue.len -= self.amount as u32;
        let mut left = NonZero::new(self.amount);
        while let Some(amount) = left {
            // SAFETY: Drain retains the admitted byte prefix and exclusive
            // owner. Positive remaining bytes prove this initialized front.
            let mut front = unsafe { crate::depot::raw::RingEdit::front_raw(&mut queue.owners) };
            let len = front.get().len();
            if let Some(tail) = Tail::positive(front.get_mut(), amount) {
                tail.commit();
                return;
            }
            left = NonZero::new(amount.get() - len.get());
            front.discard();
        }
    }
}

impl<T: buffer::raw::Segment> Drain<'_, segments::Chain<T>> {
    pub fn commit(self) {
        if let Some(prefix) = segments::Consume::new(self) {
            prefix.commit();
        }
    }
}

impl<T> AsRef<T> for Drain<'_, T> {
    fn as_ref(&self) -> &T {
        self.target
    }
}

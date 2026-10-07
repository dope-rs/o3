use std::{iter::FusedIterator, num::NonZero};

use hold::ranges;

use crate::{
    buffer,
    buffer::{hold, segments},
};

/// Views over one completely validated compact range sequence.
pub struct Iter<'plan, 'source, 'ranges, S: ?Sized, N = u32> {
    pub(super) plan: &'plan ranges::Set<'source, 'ranges, S, N>,
    pub(super) remaining: segments::Count,
}

impl<'plan, 'source, 'ranges, S: buffer::raw::Source + ?Sized, N: buffer::raw::Length>
    Iter<'plan, 'source, 'ranges, S, N>
{
    pub(super) const fn new(plan: &'plan ranges::Set<'source, 'ranges, S, N>) -> Self {
        Self {
            plan,
            remaining: plan.count(),
        }
    }
}

impl<'source, S: buffer::raw::Source + ?Sized, N: buffer::raw::Length> Iterator
    for Iter<'_, 'source, '_, S, N>
{
    type Item = buffer::View<'source, S>;

    fn next(&mut self) -> Option<Self::Item> {
        let left = NonZero::new(self.remaining.into_raw())?;
        Some(ranges::Set::advance(self, left))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.remaining.into_raw() as usize;
        (left, Some(left))
    }
}

impl<S: buffer::raw::Source + ?Sized, N: buffer::raw::Length> ExactSizeIterator
    for Iter<'_, '_, '_, S, N>
{
}
impl<S: buffer::raw::Source + ?Sized, N: buffer::raw::Length> FusedIterator
    for Iter<'_, '_, '_, S, N>
{
}

const _: () = assert!(size_of::<Iter<'static, 'static, 'static, [u8]>>() == 2 * size_of::<usize>());

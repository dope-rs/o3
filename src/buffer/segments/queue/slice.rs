use std::{mem, num::NonZero, ops};

use crate::{
    buffer,
    buffer::{
        hold, is_within, segments,
        segments::{assembly::total, queue},
        storage,
    },
    depot,
    depot::capacity,
};

/// A validated logical byte range tied to its immutable queue owner.
pub struct Slice<'a, T> {
    pub(in crate::buffer::segments::queue) queue: &'a queue::Fifo<T>,
    pub(in crate::buffer::segments::queue) start: u32,
    pub(in crate::buffer::segments::queue) end: u32,
}

impl<T> Clone for Slice<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Slice<'_, T> {}

impl<'a, T> Slice<'a, T> {
    pub(in crate::buffer::segments::queue) fn new(
        queue: &'a queue::Fifo<T>,
        range: ops::Range<usize>,
    ) -> Option<Self> {
        if !is_within(&range, queue.len()) {
            return None;
        }
        Some(Self {
            queue,
            start: range.start as u32,
            end: range.end as u32,
        })
    }

    pub(in crate::buffer::segments::queue) const fn all(queue: &'a queue::Fifo<T>) -> Self {
        Self {
            queue,
            start: 0,
            end: queue.len,
        }
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.start == self.end
    }

    #[must_use]
    pub fn get(&self, range: ops::Range<usize>) -> Option<Self> {
        if !is_within(&range, self.len()) {
            return None;
        }
        Some(Self {
            queue: self.queue,
            start: self.start + range.start as u32,
            end: self.start + range.end as u32,
        })
    }

    #[must_use]
    pub fn contiguous(&self) -> Option<&'a [u8]>
    where
        T: buffer::raw::Segment,
    {
        if self.is_empty() {
            return Some(&[]);
        }
        let bytes = self.fragments().next()?;
        (bytes.len() == self.len()).then_some(bytes)
    }

    #[must_use]
    pub const fn fragments(self) -> queue::Iter<'a, T> {
        queue::Iter::new(self)
    }

    /// Tries to retain this range in `pool` without copying payload bytes.
    ///
    /// Failure leaves every segment slot available for subsequent operations.
    pub fn try_retain(
        &self,
        pool: &segments::Pool<T>,
    ) -> Result<segments::Chain<T>, segments::Error>
    where
        T: buffer::raw::Segment,
    {
        let plan = Retain::try_new(self)?;
        pool.try_build(&plan)
    }

    /// Retains each projected owner within `policy`, copying through `retain`
    /// when its complete allocation would exceed that owner's bound.
    /// Failure releases every copied owner and reserved metadata slot.
    pub fn try_bound(
        &self,
        pool: &segments::Pool<T>,
        retain: &hold::Retention<T>,
        policy: hold::Policy,
    ) -> Result<segments::Chain<T>, queue::RetainError>
    where
        T: buffer::raw::Segment,
    {
        let (count, logical) = if self.end == self.queue.len {
            let first = if self.start == 0 {
                self.queue.cursor()
            } else {
                queue::Position {
                    location: &self.queue.cursor(),
                    start: self.start,
                    end: self.end,
                }
                .locate()
            };
            (
                segments::Count::new(self.queue.segments() as u32 - first.segment),
                first.start,
            )
        } else {
            (segments::Count::new(self.views(0, 0).count() as u32), 0)
        };
        pool.try_build(&Projected {
            source: self,
            count,
            logical,
            retain,
            policy,
        })
    }

    /// Emits at most one view per original owner, retaining the same immutable
    /// queue whose segment capacity bounds the complete traversal to u32.
    pub(in crate::buffer::segments::queue) fn views(
        &self,
        first: u32,
        logical: u32,
    ) -> impl Iterator<Item = segments::Item<buffer::View<'a, T>>> + '_
    where
        T: buffer::raw::Segment,
    {
        let logical = if self.is_empty() {
            self.end as usize
        } else {
            logical as usize
        };
        let mut entries = self.queue.entries();
        if let Some(last) = first.checked_sub(1) {
            entries.nth(last as usize);
        }
        entries
            .scan(logical, move |logical, owner| {
                if *logical >= self.end as usize {
                    return None;
                }
                let start = (self.start as usize).saturating_sub(*logical);
                let end = self.end as usize - *logical;
                *logical += owner.len().get();
                Some(segments::Item::clamp(owner.as_ref(), start, end))
            })
            .flatten()
    }
}

const _: () = assert!(mem::size_of::<Slice<'static, u64>>() == 2 * mem::size_of::<u64>());

/// Inputs whose terminal proof contains the exact owners written to the pool.
pub(in crate::buffer::segments) trait Build<T, E>:
    depot::raw::Start<
        segments::Item<T>,
        Item = Result<segments::Item<T>, E>,
        Error = E,
        Context = (),
        Output = Finished,
    >
{
}

/// Totals published only after every projected owner completed successfully.
pub(in crate::buffer::segments) struct Finished {
    len: usize,
    resident: usize,
}

impl Finished {
    pub(in crate::buffer::segments) const fn len(&self) -> usize {
        self.len
    }

    pub(in crate::buffer::segments) const fn resident(&self) -> usize {
        self.resident
    }
}

/// One measured queue slice whose shared owners match its exact totals.
pub(in crate::buffer::segments) struct Retain<'source, T> {
    source: &'source queue::Slice<'source, T>,
    count: segments::Count,
    resident: usize,
}

pub(in crate::buffer::segments) struct Projected<'source, T: buffer::raw::Segment> {
    source: &'source queue::Slice<'source, T>,
    count: segments::Count,
    logical: u32,
    retain: &'source hold::Retention<T>,
    policy: hold::Policy,
}

struct Projecting<'plan, 'source, T: buffer::raw::Segment> {
    plan: &'plan Projected<'source, T>,
    resident: usize,
}

trait Projection {
    type View;
    type Item;

    fn project(&mut self, view: Self::View) -> Self::Item;
    fn finish(self) -> Finished;
}

struct Views<I, S> {
    inner: I,
    remaining: capacity::Count,
    source: S,
}

impl<'source, T: buffer::raw::Segment> Retain<'source, T> {
    /// Retaining the immutable source preserves this u32-bounded measurement.
    pub(in crate::buffer::segments) fn try_new(
        source: &'source queue::Slice<'source, T>,
    ) -> Result<Self, segments::Error> {
        let mut count = segments::Count::EMPTY;
        let mut resident = 0usize;
        for view in source.views(0, 0) {
            count = segments::Count::new(count.into_raw().wrapping_add(1));
            resident = total::Error::resident(resident, view.source().resident())
                .map_err(segments::Error::component)?;
        }
        Ok(Self {
            source,
            count,
            resident,
        })
    }
}

impl<T: buffer::raw::Segment> Build<T, segments::Error> for &Retain<'_, T> {}

impl<T: buffer::raw::Segment> Build<T, queue::RetainError> for &Projected<'_, T> {}

// SAFETY: the original immutable slice cannot change after its exact measurement.
unsafe impl<T: buffer::raw::Segment> depot::raw::Input<segments::Item<T>> for &Retain<'_, T> {
    type Item = Result<segments::Item<T>, segments::Error>;
    type Error = segments::Error;
    type Context = ();
    type Output = Finished;

    fn count(&self) -> capacity::Count {
        capacity::Count::new(self.count.into_raw())
    }

    fn reject(self, error: capacity::Shortage) -> Self::Error {
        segments::Error::SegmentCapacity(error)
    }
}

// SAFETY: creating the traversal preserves the measured immutable source borrow.
unsafe impl<T: buffer::raw::Segment> depot::raw::Start<segments::Item<T>> for &Retain<'_, T> {
    fn start(
        self,
    ) -> (
        impl depot::raw::Exact<Item = Self::Item, Output = Self::Output>,
        (),
    ) {
        (
            Views {
                inner: self.source.views(0, 0),
                remaining: capacity::Count::new(self.count.into_raw()),
                source: self,
            },
            (),
        )
    }
}

// SAFETY: projection preserves one output or failure for each unchanged source owner.
unsafe impl<T: buffer::raw::Segment> depot::raw::Input<segments::Item<T>> for &Projected<'_, T> {
    type Item = Result<segments::Item<T>, queue::RetainError>;
    type Error = queue::RetainError;
    type Context = ();
    type Output = Finished;

    fn count(&self) -> capacity::Count {
        capacity::Count::new(self.count.into_raw())
    }

    fn reject(self, error: capacity::Shortage) -> Self::Error {
        segments::Error::SegmentCapacity(error).into()
    }
}

// SAFETY: this traversal retains the exact source, copy authority, and measured count.
unsafe impl<T: buffer::raw::Segment> depot::raw::Start<segments::Item<T>> for &Projected<'_, T> {
    /// Zero starts at owner zero; a positive origin retains the measured
    /// suffix's first owner through count and the unchanged queue borrow.
    fn start(
        self,
    ) -> (
        impl depot::raw::Exact<Item = Self::Item, Output = Self::Output>,
        (),
    ) {
        let first = if self.logical == 0 {
            0
        } else {
            self.source.queue.segments() as u32 - self.count.into_raw()
        };
        (
            Views {
                inner: self.source.views(first, self.logical),
                remaining: capacity::Count::new(self.count.into_raw()),
                source: Projecting {
                    plan: self,
                    resident: 0,
                },
            },
            (),
        )
    }
}

impl<'source, T: buffer::raw::Segment> Projection for &Retain<'source, T> {
    type View = segments::Item<buffer::View<'source, T>>;
    type Item = Result<segments::Item<T>, segments::Error>;

    fn project(&mut self, view: Self::View) -> Self::Item {
        view.try_retain().map_err(segments::Error::Retain)
    }

    fn finish(self) -> Finished {
        Finished {
            len: self.source.len(),
            resident: self.resident,
        }
    }
}

impl<'source, T: buffer::raw::Segment> Projection for Projecting<'_, 'source, T> {
    type View = segments::Item<buffer::View<'source, T>>;
    type Item = Result<segments::Item<T>, queue::RetainError>;

    fn project(&mut self, view: Self::View) -> Self::Item {
        let logical = view.len();
        let owner = if self
            .plan
            .policy
            .allows_share(logical, || view.source().resident())
        {
            view.try_retain().map_err(segments::Error::Retain)?
        } else {
            // SAFETY: this nonempty view is contained in the queue's
            // u32-bounded logical range and supplies exactly these bytes.
            let source = unsafe {
                buffer::raw::Slices::new(
                    NonZero::new_unchecked(logical.get() as u32),
                    [view.as_slice()],
                )
            };
            self.plan
                .retain
                .try_item(source)
                .map_err(queue::RetainError::Copy)?
        };
        self.resident = total::Error::resident(self.resident, owner.resident())
            .map_err(segments::Error::component)?;
        Ok(owner)
    }

    fn finish(self) -> Finished {
        Finished {
            len: self.plan.source.len(),
            resident: self.resident,
        }
    }
}

impl<I, S> Iterator for Views<I, S>
where
    I: Iterator<Item = S::View>,
    S: Projection,
{
    type Item = S::Item;

    fn next(&mut self) -> Option<Self::Item> {
        let view = self.inner.next()?;
        self.remaining = capacity::Count::new(self.remaining.raw().wrapping_sub(1));
        Some(self.source.project(view))
    }
}

// SAFETY: constructors pair the measured immutable slice with its unchanged
// iterator. Every emitted item, including an error, consumes one reservation.
unsafe impl<I, S> depot::raw::Exact for Views<I, S>
where
    I: Iterator<Item = S::View>,
    S: Projection,
{
    type Output = Finished;

    fn remaining(&self) -> capacity::Count {
        self.remaining
    }

    unsafe fn finish(self) -> Finished {
        self.source.finish()
    }
}

const _: () = {
    assert!(mem::size_of::<Retain<'static, storage::Shared>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Projected<'static, storage::Shared>>() == 5 * mem::size_of::<usize>());
};

use std::num::NonZero;

pub use error::Error;
pub use iter::Iter;

use crate::{
    base::marker::ThreadBound,
    buffer,
    buffer::{resident, segments, storage, storage::regions},
};

mod error;
mod iter;
pub mod retain;

/// One source and compact range sequence validated as a complete logical body.
#[repr(C)]
#[derive(Debug)]
pub struct Set<'source, 'ranges, S: ?Sized, N = u32> {
    source: &'source S,
    ranges: &'ranges [(N, N)],
    len: usize,
    thread: ThreadBound,
}

impl<'source, 'ranges, S: buffer::raw::Source + ?Sized, N: buffer::raw::Length>
    Set<'source, 'ranges, S, N>
{
    pub(in crate::buffer::hold) fn new(
        source: &'source S,
        ranges: &'ranges [(N, N)],
        len: usize,
    ) -> Self {
        const { assert!(N::MAX <= u32::MAX as usize) };
        Self {
            source,
            ranges,
            len,
            thread: ThreadBound::NEW,
        }
    }

    fn view(&self, start: N, end: N) -> buffer::View<'source, S> {
        // SAFETY: callers select validated entries from this same Set.
        let bytes = unsafe {
            self.source
                .as_ref()
                .get_unchecked(start.into_usize()..end.into_usize())
        };
        buffer::View {
            source: self.source,
            bytes,
        }
    }
    /// Validates each range and its complete logical length against this source.
    /// At most `u32::MAX` ranges each contribute at most `u32::MAX` bytes,
    /// so their sum fits `usize` on the supported 64-bit or wider targets.
    pub fn try_new(
        source: &'source S,
        ranges: &'ranges [(N, N)],
        expected: usize,
    ) -> Result<Self, Error<N>> {
        const { assert!(N::MAX <= u32::MAX as usize) };
        if ranges.len() > u32::MAX as usize {
            return Err(Error::Count {
                actual: ranges.len(),
            });
        }
        let len = source.as_ref().len();
        let mut actual = 0usize;
        for (index, &(start, end)) in ranges.iter().enumerate() {
            let index = index as u32;
            let first = start.into_usize();
            let last = end.into_usize();
            if first >= last {
                return Err(Error::Order { index, start, end });
            }
            if last > len {
                return Err(Error::Bounds {
                    index,
                    start,
                    end,
                    len,
                });
            }
            actual = actual.wrapping_add(last - first);
        }
        if actual != expected {
            return Err(Error::Length { expected, actual });
        }
        Ok(Self::new(source, ranges, actual))
    }

    pub fn get(&self, index: usize) -> Option<buffer::View<'source, S>> {
        let &(start, end) = self.ranges.get(index)?;
        Some(self.view(start, end))
    }

    pub fn iter(&self) -> Iter<'_, 'source, 'ranges, S, N> {
        Iter::new(self)
    }

    pub(in crate::buffer::hold::ranges) fn advance(
        iter: &mut Iter<'_, 'source, 'ranges, S, N>,
        left: NonZero<u32>,
    ) -> buffer::View<'source, S> {
        let left = left.get();
        let plan = iter.plan;
        let total = plan.count().into_raw();
        let index = (total - left) as usize;
        // SAFETY: `Iter::new` binds this private cursor to `plan` and its exact
        // validated count. Every yielded item decrements it once, so nonzero
        // `left` proves both the range index and the successor count.
        let (start, end) = unsafe { *plan.ranges.as_ptr().add(index) };
        iter.remaining = segments::Count::new(left - 1);
        plan.view(start, end)
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns the range count admitted by construction.
    pub const fn count(&self) -> segments::Count {
        segments::Count::new(self.ranges.len() as u32)
    }

    pub fn contiguous(&self) -> Option<&'source [u8]> {
        match self.ranges {
            [] => Some(&[]),
            &[(start, end)] => Some(self.view(start, end).as_slice()),
            _ => None,
        }
    }
}

impl<N: buffer::raw::Length> Set<'_, '_, storage::Shared, N> {
    /// Retains every validated range while preserving this plan for retry.
    pub fn try_retain<'d>(&self) -> Result<segments::Chain<resident::Shared<'d>>, retain::Error> {
        let actual = self.ranges.len();
        let added = crate::buffer::Residency::resident(self.source);
        let Some(resident) = actual.checked_mul(added) else {
            let added = NonZero::new(added).unwrap_or(NonZero::<usize>::MIN);
            let index = usize::MAX / added;
            return Err(retain::Error::Resident {
                index,
                current: index * added.get(),
                added: added.get(),
            });
        };
        let source = Retain {
            source: self,
            resident,
        };
        segments::Exact::try_new(&source)?.try_collect()
    }
}

struct Retain<'a, 'b, 'c, N = u32> {
    source: &'a Set<'b, 'c, storage::Shared, N>,
    resident: usize,
}

// SAFETY: the borrowed Set retains its validated ranges and exact count;
// the same Retain borrow carries their checked accounting through collection.
unsafe impl<'d, N: buffer::raw::Length> buffer::raw::Sequence<resident::Shared<'d>>
    for &Retain<'_, '_, '_, N>
{
    type Output = segments::Chain<resident::Shared<'d>>;
    type Item = Result<segments::Item<resident::Shared<'d>>, retain::Error>;
    type Error = retain::Error;
    fn owners(self) -> impl Iterator<Item = Self::Item> + buffer::raw::Total {
        let owners = self.source.iter().enumerate().map(move |(index, view)| {
            // SAFETY: Set owns this validated, strictly nonempty source range.
            let view = unsafe { regions::NonEmpty::assume(&view) };
            <storage::Shared as segments::Seal>::try_share(view)
                .map(segments::Item::from)
                .map_err(|error| retain::Error::Retain { index, error })
        });
        buffer::raw::Batch {
            owners,
            total: self,
        }
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        retain::Error::Metadata(error)
    }
}

impl<N: buffer::raw::Length> buffer::raw::Total for &Retain<'_, '_, '_, N> {
    fn count(&self) -> segments::Count {
        self.source.count()
    }

    fn len(&self) -> usize {
        self.source.len
    }

    fn resident(&self) -> usize {
        self.resident
    }
}

const _: () = {
    assert!(size_of::<Set<'static, 'static, [u8]>>() == 5 * size_of::<usize>());
    assert!(size_of::<Set<'static, 'static, storage::Shared>>() == 4 * size_of::<usize>());
    assert!(size_of::<Retain<'static, 'static, 'static>>() == 2 * size_of::<usize>());
};

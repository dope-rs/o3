use std::{marker, num::NonZero};

use crate::{
    alloc,
    depot::{
        capacity,
        heap::indexed,
        queue::{
            indexed::{lanes::runtime, planes::bound},
            links, tagged,
        },
    },
};

/// A row count derived without erasing its admitted root count.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(transparent)]
pub struct Rows<C: crate::depot::raw::Count, const N: usize>(
    pub(in crate::depot::queue::indexed::planes) C,
);

impl<C: crate::depot::raw::Count, const N: usize> Rows<C, N> {
    const fn new(count: C) -> Self {
        const {
            assert!(N != 0 && N <= u32::MAX as usize);
            assert!(C::MAX as u64 * N as u64 <= u32::MAX as u64);
        }
        Self(count)
    }
}

// SAFETY: the private constructor retains the immutable root count and admits its exact product.
unsafe impl<C: crate::depot::raw::Count, const N: usize> crate::depot::raw::Count for Rows<C, N> {
    const MAX: u32 = {
        assert!(N != 0 && N <= u32::MAX as usize);
        assert!(C::MAX as u64 * N as u64 <= u32::MAX as u64);
        (C::MAX as u64 * N as u64) as u32
    };
    const MIN: u32 = {
        let _ = Self::MAX;
        (C::MIN as u64 * N as u64) as u32
    };

    fn count(self) -> capacity::Count {
        capacity::Count::new((self.0.count().raw() as u64 * N as u64) as u32)
    }
}

/// The original power-of-two bucket count, retaining its static root bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Buckets<C: crate::depot::raw::Count, const MULTIPLIER: usize> {
    count: NonZero<u32>,
    root: marker::PhantomData<C>,
}

impl<C: crate::depot::raw::Count, const MULTIPLIER: usize> Buckets<C, MULTIPLIER> {
    fn new(root: C) -> Self {
        const {
            assert!(C::MIN != 0);
            assert!(MULTIPLIER != 0 && MULTIPLIER <= u32::MAX as usize);
            assert!((C::MAX as u64 * MULTIPLIER as u64).next_power_of_two() <= u32::MAX as u64);
        }
        let count = (root.count().raw() as u64 * MULTIPLIER as u64).next_power_of_two() as u32;
        Self {
            count: NonZero::<u32>::MIN.saturating_add(count - 1),
            root: marker::PhantomData,
        }
    }
}

// SAFETY: construction retains a nonzero power of two within this immutable static bound.
unsafe impl<C: crate::depot::raw::Count, const MULTIPLIER: usize> crate::depot::raw::Count
    for Buckets<C, MULTIPLIER>
{
    const MAX: u32 = {
        assert!(MULTIPLIER != 0 && MULTIPLIER <= u32::MAX as usize);
        let count = (C::MAX as u64 * MULTIPLIER as u64).next_power_of_two();
        assert!(count <= u32::MAX as u64);
        count as u32
    };
    const MIN: u32 = 1;

    fn count(self) -> capacity::Count {
        capacity::Count::new(self.count.get())
    }
}

/// Priorities and one fixed-width companion store sharing a current prefix.
pub struct Index<
    P: Ord,
    C: crate::depot::raw::Count + alloc::raw::Extent,
    T,
    const N: usize,
    const MULTIPLIER: usize,
> {
    pub(in crate::depot::queue::indexed::planes) priority: indexed::Min<P, C>,
    pub(in crate::depot::queue::indexed::planes) records:
        runtime::Fifo<T, Rows<C, N>, Buckets<C, MULTIPLIER>, links::List>,
}

impl<
    P: Ord,
    C: crate::depot::raw::Count + alloc::raw::Extent,
    T,
    const N: usize,
    const MULTIPLIER: usize,
> Index<P, C, T, N, MULTIPLIER>
{
    pub fn reserve(
        capacity: C,
    ) -> Result<runtime::Fifo<T, Rows<C, N>, Buckets<C, MULTIPLIER>, links::List>, alloc::Error>
    {
        runtime::Fifo::try_new(tagged::Domain::bounded(
            Rows::new(capacity),
            Buckets::new(capacity),
        ))
    }
}
/// One exclusive companion loan. It proves geometry, not row occupancy.
pub struct Side<
    'storage,
    'id,
    T,
    C: crate::depot::raw::Count,
    const N: usize,
    const MULTIPLIER: usize,
> {
    pub(in crate::depot::queue::indexed::planes) fifo:
        &'storage mut runtime::Fifo<T, Rows<C, N>, Buckets<C, MULTIPLIER>, links::List>,
    pub(in crate::depot::queue::indexed::planes) brand: marker::PhantomData<capacity::Domain<'id>>,
}

impl<'id, T, C: crate::depot::raw::Count, const N: usize, const MULTIPLIER: usize>
    Side<'_, 'id, T, C, N, MULTIPLIER>
{
    pub fn bucket(&self, hash: u64) -> impl Iterator<Item = (bound::ActiveKey<'id>, &T)> + '_ {
        self.fifo.bucket(hash).into_iter().map(|live| {
            let row = live.key().get() / N;
            // SAFETY: this actual Fifo has exactly active * N nodes and cannot be resized.
            let index = unsafe { links::Index::new_usize(row) };
            let key = bound::ActiveKey(capacity::Key::new(unsafe {
                crate::depot::raw::Proof::new(index)
            }));
            (key, live.into())
        })
    }

    pub fn group(
        &mut self,
        key: bound::ActiveKey<'id>,
    ) -> runtime::Group<'_, T, Rows<C, N>, Buckets<C, MULTIPLIER>, links::List, N> {
        // SAFETY: the same root issued this active key and retains exactly N nodes per row.
        let first = unsafe { links::Index::new_usize(key.index() * N) };
        unsafe { runtime::Group::new(self.fifo, first) }
    }

    pub(in crate::depot::queue::indexed::planes) fn clear(
        mut self,
        key: bound::ActiveKey<'id>,
    ) -> links::Index {
        self.group(key).clear();
        key.0.into_inner()
    }
}

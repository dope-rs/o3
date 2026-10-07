use std::{convert, num};

use crate::{
    depot,
    depot::{index::Initialized, map::compact},
};

/// Publishes the exact admitted record through its retained directory owner.
pub trait Insert: Sized {
    type Output;
    fn insert(self) -> Self::Output;
}

impl<Q: compact::raw::Insert> Insert for Q {
    type Output = <Q as compact::raw::Insert>::Output;
    fn insert(self) -> Self::Output {
        // SAFETY: Insert retains a nonempty directory and its admitted spare slot.
        unsafe { compact::raw::Store::with(self, |store| store.insert()) }
    }
}

/// The record plane of a borrowed directory owner.
///
/// Live keys name initialized records below the retained owner's the record domain.
/// Directory bucket indices are a different plane and do not establish this proof.
#[derive(Debug)]
pub enum Payload {}

pub mod raw;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Start { actual: usize, maximum: usize },
    Capacity { actual: usize, maximum: usize },
}

const _: () = assert!(size_of::<Error>() == 3 * size_of::<usize>());

#[derive(Clone, Copy)]
#[repr(u8)]
enum Width {
    One = 1,
    Two = 2,
    Four = 4,
}

impl Width {
    const fn new(domain: num::NonZero<u32>) -> Self {
        match domain.get() - 1 {
            0..=0xff => Self::One,
            0x100..=0xffff => Self::Two,
            0x10000.. => Self::Four,
        }
    }
}

const MAX_CAPACITY: usize = 7 << 28;
const WIDTH: usize = 32;
const EMPTY: u8 = 1 << 7;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    start: u32,
    buckets: u32,
    domain: num::NonZero<u32>,
}

impl Layout {
    pub fn try_new(capacity: usize, start: usize) -> Result<(Self, usize), compact::Error> {
        let maximum = u32::MAX as usize;
        if start > maximum {
            return Err(compact::Error::Start {
                actual: start,
                maximum,
            });
        }
        if capacity > MAX_CAPACITY {
            return Err(compact::Error::Capacity {
                actual: capacity,
                maximum: MAX_CAPACITY,
            });
        }
        let Some(domain) = num::NonZero::new(capacity as u32) else {
            return Ok((
                Self {
                    start: start as u32,
                    buckets: 0,
                    domain: num::NonZero::<u32>::MIN,
                },
                start,
            ));
        };
        let minimum = (capacity * 8).div_ceil(7).max(WIDTH);
        let buckets = minimum.next_power_of_two();
        let layout = Self {
            start: start as u32,
            buckets: buckets as u32,
            domain,
        };
        Ok((layout, layout.end()))
    }

    const fn end(self) -> usize {
        if self.buckets == 0 {
            return self.start as usize;
        }
        self.plane() + self.buckets as usize * compact::Width::new(self.domain) as usize
    }

    pub const fn byte(self, offset: usize) -> u8 {
        let start = self.start as usize;
        if self.buckets != 0 && offset >= start && offset < start + self.control_len() {
            EMPTY
        } else {
            0
        }
    }

    const fn control_len(self) -> usize {
        self.buckets as usize + WIDTH
    }

    const fn plane(self) -> usize {
        self.start as usize + self.control_len()
    }
}

/// `N` stacked directories whose control mirrors are merged over one arena.
///
/// A merged control byte admits an offset in some directory plane,
/// never the initialized record named by a bucket in that plane.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Planes<const N: usize> {
    layouts: [Layout; N],
}

impl<const N: usize> Planes<N> {
    /// Stacks one directory per capacity and returns the retained end offset.
    pub fn try_new(counts: [usize; N], start: usize) -> Result<(Self, usize), compact::Error> {
        let (seed, _) = Layout::try_new(0, start)?;
        let mut layouts = [seed; N];
        let mut end = start;
        for (layout, capacity) in layouts.iter_mut().zip(counts) {
            let (plane, next) = Layout::try_new(capacity, end)?;
            *layout = plane;
            end = next;
        }
        Ok((Self { layouts }, end))
    }

    /// The directory owning the statically checked plane `INDEX`.
    pub const fn layout<const INDEX: usize>(&self) -> Layout {
        const { assert!(INDEX < N, "plane") };
        self.layouts[INDEX]
    }

    /// Merges every plane's control byte at one arena offset.
    pub fn byte(&self, offset: usize) -> u8 {
        let mut merged = 0;
        for layout in &self.layouts {
            merged = merged.max(layout.byte(offset));
        }
        merged
    }
}

const _: () = {
    assert!(size_of::<Layout>() == 12);
    assert!(align_of::<Layout>() == 4);
    assert!(size_of::<Option<Layout>>() == 12);
    assert!(size_of::<Planes<2>>() == 2 * size_of::<Layout>());
};

/// Retains the queried owner whether a candidate matches or the search ends.
pub enum Entry<Q: compact::raw::Read> {
    Occupied(compact::Found<Q>),
    Vacant(Q),
}

/// A live, bounded record in the borrowed source owner.
pub struct Value<'a, Q: compact::raw::Read> {
    owner: &'a Q,
    index: u32,
}

impl<'a, Q: compact::raw::Read> Value<'a, Q> {
    /// Consumes an initialized, bounded record in this exact source's payload plane.
    pub fn new(live: depot::raw::Proof<(&'a Q, u32), Initialized<compact::Payload>>) -> Self {
        let (owner, index) = live.into_inner();
        Self { owner, index }
    }

    pub const fn owner(&self) -> &'a Q {
        self.owner
    }

    pub const fn index(&self) -> usize {
        self.index as usize
    }

    pub fn capacity(&self) -> num::NonZero<u32> {
        self.owner.layout().domain
    }
}

/// Owns a located member and the source borrow that keeps its bucket valid.
pub struct Found<Q: compact::raw::Read> {
    owner: Q,
    bucket: num::NonZero<u32>,
    payload: u32,
}

impl<Q: compact::raw::Member> Found<Q> {
    pub fn new(owner: Q) -> Self {
        // SAFETY: Member retains a live record in this nonempty directory.
        unsafe { compact::raw::Store::with(owner, |store| store.locate()) }
    }
}

impl<Q: compact::raw::Read> Found<Q> {
    pub fn value(&self) -> compact::Value<'_, Q> {
        compact::Value::new(unsafe { depot::raw::Proof::new((&self.owner, self.payload)) })
    }

    /// Completes the lookup and returns its original owner.
    pub fn into_owner(self) -> Q {
        self.owner
    }
}

impl<Q: compact::raw::Remove> Found<Q> {
    pub fn remove(self) -> Q::Output {
        compact::raw::Store::remove(self)
    }
}

/// The source still owns this live record, and its directory no longer indexes it.
pub struct Removed<Q: compact::raw::Remove> {
    owner: Q,
    payload: u32,
}

impl<Q: compact::raw::Remove> Removed<Q> {
    pub fn value(&self) -> compact::Value<'_, Q> {
        compact::Value::new(unsafe { depot::raw::Proof::new((&self.owner, self.payload)) })
    }
}

/// Safe directory operations over a source whose query contract retains its storage proof.
pub trait Access: compact::raw::Query {
    fn find(self, hash: u64, key: &Self::Item) -> Option<compact::Found<Self>>
    where
        Self: compact::raw::Remove,
        Self::Item: Eq,
    {
        compact::raw::Store::with_layout(self, |store| store.find(hash, key), |_| None)
    }

    fn scan(self, hash: u64, visit: impl FnMut(Self::Item)) -> Self {
        compact::raw::Store::with_layout(self, |store| store.scan(hash, visit), convert::identity)
    }

    fn entry(self, hash: u64, key: &Self::Item) -> compact::Entry<Self>
    where
        Self::Item: Eq,
    {
        compact::raw::Store::with_layout(
            self,
            |store| store.entry(hash, key),
            compact::Entry::Vacant,
        )
    }
}

impl<Q: compact::raw::Query> Access for Q {}

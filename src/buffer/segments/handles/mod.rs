use std::{mem, num::NonZero};

use handle::Handle;
use header::Header;
use pool::Pool;

use crate::depot::arena::linked::{fifo::ends, slots};

pub(in crate::buffer::segments) mod handle;
pub(in crate::buffer::segments) mod header;
pub(in crate::buffer::segments) mod pool;

use crate::{
    alloc,
    buffer::{resident, segments, storage},
};

/// Metadata owned by a multi key, including a detached-front transient.
#[derive(Debug)]
#[repr(C)]
pub(in crate::buffer::segments) struct Descriptor {
    pub(in crate::buffer::segments) links: ends::Exclusive<slots::Index>,
    pub(in crate::buffer::segments) count: NonZero<u32>,
    pub(in crate::buffer::segments) len: usize,
    pub(in crate::buffer::segments) resident_bytes: usize,
}

impl Descriptor {
    pub(in crate::buffer::segments) const fn new(
        links: ends::Exclusive<slots::Index>,
        count: NonZero<u32>,
        len: usize,
        resident: usize,
    ) -> Self {
        Self {
            links,
            count,
            len,
            resident_bytes: resident,
        }
    }
}

struct Parts<T> {
    bundle: Fields<T>,
    nodes: slots::Capacity<segments::Item<T>>,
    descriptors: slots::Capacity<Descriptor>,
}

impl<T> Parts<T> {
    fn try_reserve(
        current: segments::Count,
        required: segments::Count,
    ) -> Result<Self, crate::alloc::Overflow> {
        let count = required.into_raw() as usize;
        let capacity = alloc::raw::Layout::<slots::Node<segments::Item<T>>>::reserve(
            current.into_raw() as usize,
            count,
        )
        .min(segments::Plan::max_segments());
        let plan = segments::Plan {
            segments: capacity,
            chains: usize::from(required.is_many()),
        };
        Self::try_new(plan).or_else(|error| {
            if capacity == count {
                return Err(error);
            }
            Self::try_new(segments::Plan {
                segments: count,
                ..plan
            })
        })
    }

    fn try_new(plan: segments::Plan) -> Result<Self, crate::alloc::Overflow> {
        if plan.segments > segments::Plan::max_segments() || plan.chains > plan.segments / 2 {
            return Err(crate::alloc::Overflow);
        }
        let nodes = slots::Capacity::new(plan.segments)?;
        let desc = slots::Capacity::new(plan.chains)?;
        let bundle = alloc::raw::Bundle::try_root::<Header<T>>(1)?
            .try_tail::<slots::Node<segments::Item<T>>>(nodes.len())?
            .try_tail::<slots::Node<Descriptor>>(desc.len())?;
        Ok(Self {
            bundle,
            nodes,
            descriptors: desc,
        })
    }
}

/// Portable tagged index for Empty, Single, and Multi states.
#[derive(Debug)]
#[repr(transparent)]
pub(in crate::buffer::segments) struct Key(usize);

pub(in crate::buffer::segments) enum State {
    Empty,
    Single(slots::Index),
    Multi(slots::Index),
}

/// A decoded key whose retained owner is known to be nonempty.
pub(in crate::buffer::segments) enum Occupied {
    Single(slots::Index),
    Multi(slots::Index),
}

impl Key {
    const MULTI: usize = 1usize << (usize::BITS - 1);

    pub(in crate::buffer::segments) const EMPTY: Self = Self(0);

    pub(in crate::buffer::segments) const fn is_empty(&self) -> bool {
        self.0 == 0
    }

    pub(in crate::buffer::segments) const fn state(&self) -> State {
        if self.0 == 0 {
            return State::Empty;
        }
        // SAFETY: the empty key was excluded above.
        match unsafe { self.occupied() } {
            Occupied::Single(index) => State::Single(index),
            Occupied::Multi(index) => State::Multi(index),
        }
    }

    /// # Safety
    /// This key retains a live single node or multi descriptor, never Empty.
    pub(in crate::buffer::segments) const unsafe fn occupied(&self) -> Occupied {
        let index = slots::Index::new(((self.0 & !Self::MULTI) - 1) as u32);
        if self.0 & Self::MULTI == 0 {
            Occupied::Single(index)
        } else {
            Occupied::Multi(index)
        }
    }

    pub(in crate::buffer::segments) fn single(index: slots::Index) -> Self {
        let value = index.raw() as usize + 1;
        Self(value)
    }

    pub(in crate::buffer::segments) fn multi(index: slots::Index) -> Self {
        let value = index.raw() as usize + 1;
        Self(Self::MULTI | value)
    }

    /// # Safety
    /// This key is in the single state.
    pub(in crate::buffer::segments) const unsafe fn single_index(&self) -> slots::Index {
        slots::Index::new((self.0 - 1) as u32)
    }

    /// # Safety
    /// This key is in the multi state.
    pub(in crate::buffer::segments) const unsafe fn multi_index(&self) -> slots::Index {
        slots::Index::new(((self.0 & !Self::MULTI) - 1) as u32)
    }
}

pub(in crate::buffer::segments) fn nodes<T, H: Handle<T>>(
    handle: &H,
) -> &slots::Store<segments::Item<T>> {
    &handle.header().nodes
}

pub(in crate::buffer::segments) fn descriptors<'handle, T: 'handle, H: Handle<T>>(
    handle: &'handle H,
) -> &'handle slots::Store<Descriptor> {
    &handle.header().descriptors
}

const _: () = {
    assert!(mem::size_of::<Key>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Pool<()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Pool<()>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<&'static Header<()>>() == mem::size_of::<usize>());
    assert!(!mem::needs_drop::<Key>());
    assert!(!mem::needs_drop::<&'static Header<()>>());
    assert!(
        size_of::<slots::Node<segments::Item<storage::Shared>>>()
            == size_of::<slots::Node<storage::Shared>>()
    );
    assert!(
        align_of::<slots::Node<segments::Item<storage::Shared>>>()
            == align_of::<slots::Node<storage::Shared>>()
    );
    assert!(
        size_of::<slots::Node<segments::Item<resident::Shared<'static>>>>()
            == size_of::<slots::Node<resident::Shared<'static>>>()
    );
    assert!(
        align_of::<slots::Node<segments::Item<resident::Shared<'static>>>>()
            == align_of::<slots::Node<resident::Shared<'static>>>()
    );
};

type Fields<T> = alloc::raw::Bundle<
    alloc::raw::Tail<
        alloc::raw::Tail<alloc::raw::Layout<Header<T>>, slots::Node<segments::Item<T>>>,
        slots::Node<Descriptor>,
    >,
>;

pub(in crate::buffer::pool) mod lease;
use std::{marker, mem, ops, ptr};

use pool::Lease;

use crate::{
    buffer,
    buffer::{
        pool,
        pool::{owners, state},
        resident::{AllocError, staging, write::Cause},
        storage,
        write::{consume, consume::Error},
    },
};

pub(in crate::buffer::pool) mod append;
pub(in crate::buffer::pool) mod complete;
pub(in crate::buffer::pool) mod copy;
pub(in crate::buffer::pool) mod frozen;

/// A writable span retaining an initialized slot lease.
#[derive(Debug)]
#[repr(transparent)]
pub struct Span<K: crate::buffer::raw::Backing = owners::Plain, M = pool::Owned, S = state::Uninit>
{
    pub(in crate::buffer::pool) lease: Lease<K, M, S>,
}

impl<K: crate::buffer::raw::Backing, M, S> ops::Deref for Span<K, M, S> {
    type Target = Lease<K, M, S>;
    fn deref(&self) -> &Self::Target {
        &self.lease
    }
}

impl<K: crate::buffer::raw::Backing, M, S> ops::DerefMut for Span<K, M, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.lease
    }
}

impl<K: crate::buffer::raw::Backing, S> Span<K, pool::Owned, S> {
    pub fn recycle(self) {
        drop(self);
    }

    pub(in crate::buffer::pool) fn acquire(core: owners::Ptr<K, S>) -> Option<Self> {
        let slots = core;
        let claim = owners::Claim::new(slots)?;
        Some(Self::new(claim.take(), pool::Owned))
    }

    pub(in crate::buffer::pool) fn from_seed(seed: owners::Seed<K, S>) -> Self {
        let (core, active) = seed.take();
        owners::Header::release(core);
        Self::new(active, pool::Owned)
    }
}

impl<'pool, K: crate::buffer::raw::Backing, S> Span<K, pool::Borrowed<'pool>, S> {
    pub(in crate::buffer::pool) fn borrow(core: &'pool owners::Ptr<K, S>) -> Option<Self> {
        let slots = *core;
        let claim = owners::Claim::new(slots)?;
        Some(Self::new(claim.take(), pool::Borrowed(marker::PhantomData)))
    }
}

impl<K: crate::buffer::raw::Backing, M, S> Span<K, M, S> {
    pub(in crate::buffer) fn into_edge(self) -> owners::Edge<K, M> {
        let this = mem::ManuallyDrop::new(self);
        owners::Edge::new(this.entry().owner())
    }

    pub(in crate::buffer::pool) fn new(active: owners::Active<K, S>, root: M) -> Self {
        let active = mem::ManuallyDrop::new(active);
        Self {
            lease: Lease {
                slots: active.slots,
                key: active.key,
                len: 0,
                root,
            },
        }
    }
}

// SAFETY: Span tracks exactly its initialized prefix.
unsafe impl<K: crate::buffer::raw::Backing, M, S> pool::raw::Init for Span<K, M, S> {
    type Backing = K;
    type Root = M;
    type State = S;

    fn into_parts(self) -> (Span<K, M, S>, usize, usize) {
        let len = self.len();
        (self, 0, len)
    }
}

impl<'d> Span<owners::Resident<'d>> {
    pub(in crate::buffer) fn try_staged<I: buffer::raw::SliceExt>(
        reserve: staging::Reserve<&staging::Pool<'d>, (&[u8], owners::Measured<I>)>,
    ) -> Result<Self, AllocError> {
        let (pool, capacity, (first, second)) = reserve.into_parts();
        let len = first.len() + second.len();
        let layout = pool::NonEmpty::new(capacity);
        let seed =
            owners::Block::<owners::Accounted, state::Uninit>::try_seed(layout, pool.budget())?;
        let span = Span::from_seed(seed);
        let span = buffer::raw::Copy::copy(pool::CopyIn {
            span,
            sources: (first, second.into_input()),
            len,
        });
        Ok(span)
    }
}

const _: () = {
    assert!(size_of::<Span>() == 2 * size_of::<usize>());
    assert!(size_of::<Span<owners::Plain, pool::Borrowed<'static>>>() == 2 * size_of::<usize>());
    assert!(size_of::<Span<owners::Resident<'static>>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Span<owners::Plain, pool::Owned, state::Initialized>>() == 2 * size_of::<usize>()
    );
};

impl<K: crate::buffer::raw::Backing, M, S> AsRef<[u8]> for Span<K, M, S> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl<S: state::State, L> pool::Span<owners::Plain<L>, pool::Owned, S> {
    #[must_use]
    pub fn freeze(self) -> pool::Span<owners::Plain<L>, pool::Owned, pool::Frozen> {
        let this = mem::ManuallyDrop::new(self);
        Span {
            lease: Lease {
                slots: this.slots.erase_state(),
                key: this.key,
                len: this.len,
                root: this.root,
            },
        }
    }
}

impl<'pool, S: state::State, L> pool::Span<owners::Plain<L>, pool::Borrowed<'pool>, S> {
    #[must_use]
    pub fn freeze(self) -> storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'pool>>> {
        pool::raw::Init::into_shared(self)
    }
}

impl<L, M> pool::Span<owners::Plain<L>, M, state::Uninit> {
    pub fn try_append<'a, 'source>(
        &'a mut self,
        source: &'source [u8],
    ) -> Result<pool::Append<'a, &'source [u8], Self>, Error> {
        let input = owners::Measured::slice(source);
        pool::Append::try_lease(self, input)
    }
}

impl pool::Span<owners::Plain<pool::Layout>, pool::Owned, state::Uninit> {
    /// Allocates one independently owned writable slot.
    pub fn try_new(cap: usize) -> Result<Self, pool::CreateError> {
        let layout = pool::NonEmpty::one(cap).map_err(pool::CreateError::Layout)?;
        let seed = owners::Block::<owners::Plain<pool::Layout>, state::Uninit>::try_seed(layout)
            .map_err(pool::CreateError::Exhausted)?;
        Ok(pool::Span::from_seed(seed))
    }
}

impl<L> pool::Span<owners::Plain<L>, pool::Owned, state::Initialized> {
    pub fn output(&mut self) -> buffer::raw::Output<'_, L> {
        buffer::raw::Output::new(self)
    }
}

/// One acquired resident-pool backing without a duplicate pool owner.
impl<'d> pool::Span<owners::Resident<'d>> {
    pub(in crate::buffer) fn staged<I: buffer::raw::SliceExt>(
        reserve: staging::Reserve<Self, owners::Measured<I>>,
    ) -> Self {
        let (mut cursor, _, src) = reserve.into_parts();
        let start = cursor.len();
        let target = cursor.entry().data();
        let end = start + src.len();
        buffer::raw::Copy::copy(Append {
            cursor: &mut cursor,
            sources: src.into_input(),
            start,
            target,
            end,
        });
        cursor
    }

    pub(in crate::buffer) fn try_extend<'a, I: buffer::raw::SliceExt>(
        slot: staging::Slot<'a, 'd>,
        slab: Option<&pool::Pool<state::Uninit, owners::Resident<'d>>>,
        input: &owners::Measured<I>,
    ) -> Result<(), Cause> {
        let added = input.len();
        let src = input.input();
        let (cursor, head) = slot.into_parts();
        let capacity = cursor.capacity().get() as usize;
        let tail = match consume::Error::check(added, capacity - cursor.len()) {
            Ok(()) => {
                let start = cursor.len();
                let target = cursor.entry().data();
                buffer::raw::Copy::copy(Append {
                    cursor,
                    sources: src,
                    start,
                    target,
                    end: start + added,
                });
                return Ok(());
            }
            Err(error) => error,
        };
        let len = cursor.len() - *head as usize;
        consume::Error::check(added, capacity - len).map_err(Cause::Capacity)?;
        if cursor.entry().is_unique() {
            let data = cursor.entry().data();
            let start = mem::take(head) as usize;
            // SAFETY: the exclusive slot is unique and both ranges remain within its initialized extent.
            unsafe { ptr::copy(data.add(start), data, len) };
            buffer::raw::Copy::copy(Append {
                cursor,
                sources: src,
                start: len,
                target: data,
                end: len + added,
            });
            return Ok(());
        }
        let Some(slab) = slab else {
            return Err(Cause::Shared(tail));
        };
        // Slot and slab retain the same warm pool; the suffix check above proves the copy fits.
        let Some(mut next) = slab.acquire() else {
            return Err(Cause::Rotation(tail));
        };
        // SAFETY: the exclusive source borrow retains its exact suffix through the copy.
        let bytes = unsafe { cursor.as_slice().get_unchecked(*head as usize..) };
        let target = next.entry().data();
        buffer::raw::Copy::copy(Append {
            cursor: &mut next,
            sources: (bytes, src),
            start: 0,
            target,
            end: bytes.len() + added,
        });
        *head = 0;
        *cursor = next;
        Ok(())
    }
}

struct Append<'a, 'd, I> {
    cursor: &'a mut pool::Span<owners::Resident<'d>>,
    sources: I,
    start: usize,
    target: *mut u8,
    end: usize,
}

// SAFETY: each admitted edit retains the exact writable suffix and disjoint sources.
unsafe impl<I: buffer::raw::SliceExt> buffer::raw::Copy for Append<'_, '_, I> {
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (&self.sources, self.target, self.start)
    }

    unsafe fn publish(self) {
        self.cursor.len = self.end as u32;
    }
}

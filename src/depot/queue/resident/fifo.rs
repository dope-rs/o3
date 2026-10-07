//! Bounded payloads with owner-bound lanes and linear FIFO registrations.

use std::{marker, mem, num, ptr};

use crate::depot::queue::resident;

/// Bounded FIFO value slots with a constructor-proved lane extent.
pub struct Fifo<W, L = num::NonZero<usize>> {
    entries: resident::Lanes<resident::raw::Entry<W>, resident::Plain, L>,
    draining: resident::Draining,
}

/// One lane borrowed from its exact immutable FIFO owner.
#[derive(Clone, Copy)]
pub struct Lane<'a, W, L = num::NonZero<usize>> {
    fifo: &'a Fifo<W, L>,
    lane: resident::Lane,
}

/// The invariant identity of one FIFO's immutable lane extent.
pub struct Scope<'scope> {
    lanes: num::NonZero<usize>,
    brand: marker::PhantomData<fn(&'scope ()) -> &'scope ()>,
}

impl resident::Extent for Scope<'_> {
    fn count(self) -> num::NonZero<usize> {
        self.lanes
    }
}

/// A lane admitted by the only FIFO in its scope.
#[derive(Clone, Copy)]
pub struct Index<'scope> {
    position: num::NonZero<u32>,
    brand: marker::PhantomData<fn(&'scope ()) -> &'scope ()>,
}

/// A positive u16 lane retaining its FIFO's invariant scope and prefix extent.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Compact<'scope> {
    value: num::NonZero<u16>,
    brand: marker::PhantomData<fn(&'scope ()) -> &'scope ()>,
}

impl<'scope> Compact<'scope> {
    pub const fn get(self) -> num::NonZero<u16> {
        self.value
    }

    pub const fn into_index(self) -> Index<'scope> {
        Index {
            position: num::NonZero::<u32>::MIN.saturating_add(self.value.get() as u32),
            brand: self.brand,
        }
    }

    /// Advances within the positive prefix admitted by this upper lane.
    pub const fn after(self, previous: Option<Self>) -> Option<Self> {
        let previous = match previous {
            Some(index) => index.value.get(),
            None => 0,
        };
        if previous >= self.value.get() {
            return None;
        }
        Some(Self {
            value: num::NonZero::<u16>::MIN.saturating_add(previous),
            brand: self.brand,
        })
    }
}

impl<'scope> TryFrom<Index<'scope>> for Compact<'scope> {
    type Error = num::TryFromIntError;

    fn try_from(index: Index<'scope>) -> Result<Self, Self::Error> {
        Ok(Self {
            value: num::NonZero::try_from(u16::try_from(index.position.get() - 1)?)?,
            brand: index.brand,
        })
    }
}

/// Linear ownership of one slot, including after its value has been taken.
#[repr(transparent)]
pub struct Registration<'target, W, L = num::NonZero<usize>> {
    lease: resident::Lease<'target, Fifo<W, L>>,
}

/// A registration whose current queue binding rejected an update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a rejected registration must be handled"]
pub enum Reject {
    Active { index: usize },
    Draining { index: usize },
    Full { index: usize },
}

impl<W, L> Fifo<W, L> {
    pub fn try_new(capacity: usize, lanes: L) -> Result<Self, resident::Error>
    where
        L: resident::Extent,
    {
        Ok(Self {
            entries: resident::Lanes::try_new(capacity, lanes)?,
            draining: resident::Draining::default(),
        })
    }

    /// Borrows lane zero, retained by every constructor's nonzero extent.
    pub fn first(&self) -> Lane<'_, W, L> {
        Lane {
            fifo: self,
            lane: resident::Lane::new::<1, 0>(),
        }
    }

    pub fn take_next(&self) -> Option<W> {
        if self.is_draining() {
            return None;
        }
        let entry = self.entries.take::<true>()?;
        // SAFETY: the removed linked entry was initialized by try_keep and
        // remains reserved by its registration while this value is consumed.
        Some(unsafe { entry.take() })
    }

    pub fn has_waiters(&self) -> bool {
        self.entries.has_waiters()
    }

    pub fn begin_drain(&self) {
        self.draining.begin();
    }

    pub fn is_draining(&self) -> bool {
        self.draining.is_active()
    }

    /// Takes the next value in drain order and ends draining at quiescence.
    pub fn drain_next(&self) -> resident::Drain<W> {
        self.draining.drain(|| {
            let entry = self.entries.take::<false>()?;
            // SAFETY: only initialized values are linked; take removes
            // this link before the one owning value is consumed.
            Some(unsafe { entry.take() })
        })
    }
}

impl<W> Fifo<W> {
    pub fn try_lane(
        &self,
        index: usize,
    ) -> Result<Lane<'_, W>, resident::Bounds<num::NonZero<u32>>> {
        self.entries
            .try_lane(index)
            .map(|lane| Lane { fifo: self, lane })
    }

    /// Creates one FIFO and borrows its source under a fresh invariant scope.
    pub fn try_with<S, R>(
        capacity: usize,
        lanes: num::NonZero<usize>,
        source: S,
        run: impl for<'scope> FnOnce(&'scope S, Fifo<W, Scope<'scope>>) -> R,
    ) -> Result<R, resident::Error> {
        let scope = Scope {
            lanes,
            brand: marker::PhantomData,
        };
        let fifo = Fifo::try_new(capacity, scope)?;
        Ok(run(&source, fifo))
    }
}

impl<'scope, W> Fifo<W, Scope<'scope>> {
    pub fn try_index(
        &self,
        index: usize,
    ) -> Result<Index<'scope>, resident::Bounds<num::NonZero<u32>>> {
        Ok(Index {
            position: self.entries.try_lane(index)?.index().position(),
            brand: marker::PhantomData,
        })
    }

    /// Borrows a lane admitted by this scope's only FIFO.
    pub fn lane(&self, index: Index<'scope>) -> Lane<'_, W, Scope<'scope>> {
        Lane {
            fifo: self,
            lane: resident::Lane::from_pos(index.position),
        }
    }
}

impl<W, const N: usize> Fifo<W, resident::Fixed<N>> {
    /// Borrows a lane proved to exist by this FIFO's fixed extent.
    pub fn lane<const I: usize>(&self) -> Lane<'_, W, resident::Fixed<N>> {
        Lane {
            fifo: self,
            lane: resident::Lane::new::<N, I>(),
        }
    }
}

impl<'a, W, L> Lane<'a, W, L> {
    pub fn try_keep(self, slot: &mut Registration<'a, W, L>, value: W) -> Result<(), Reject>
    where
        W: Copy,
    {
        let fifo = self.fifo;
        let lane = self.lane;
        let index = lane.index().get();
        if fifo.is_draining() {
            return Err(Reject::Draining { index });
        }
        match slot.lease.active() {
            Some((active, key)) => {
                if !ptr::eq(active, fifo) {
                    return Err(Reject::Active { index });
                }
                let (next, entry) = active.entries.relink_back(*key, lane);
                entry.initialize(value);
                *key = next;
            }
            None => {
                let (key, entry) = fifo.entries.try_push(lane, Reject::Full { index })?;
                entry.initialize(value);
                slot.lease.set(fifo, key);
            }
        }
        Ok(())
    }

    pub fn take_one(self) -> resident::Take<W> {
        let Some(entry) = self.fifo.entries.pop(self.lane) else {
            return resident::Take::Empty;
        };
        // SAFETY: pop removes one initialized linked value from this exact
        // owner; the registration keeps its now-empty slot reserved.
        resident::Take::Item(unsafe { entry.take() })
    }
}

impl<W, L> Default for Registration<'_, W, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W, L> Registration<'_, W, L> {
    pub const fn new() -> Self {
        Self {
            lease: resident::Lease::new(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.lease.is_active()
    }
}

impl<W, L> resident::Target for Fifo<W, L> {
    type Key = resident::Key;

    fn retire(&self, key: Self::Key) {
        // Only Copy values enter this FIFO; unlinking ends their ownership.
        self.entries.remove(key);
    }
}

const _: () = {
    assert!(mem::size_of::<Registration<'static, usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::align_of::<Registration<'static, usize>>() == mem::align_of::<usize>());
    assert!(mem::size_of::<Index<'static>>() == mem::size_of::<u32>());
    assert!(mem::size_of::<Compact<'static>>() == mem::size_of::<u16>());
    assert!(mem::size_of::<Option<Compact<'static>>>() == mem::size_of::<u16>());
    assert!(mem::size_of::<Fifo<usize, Scope<'static>>>() == mem::size_of::<Fifo<usize>>());
};

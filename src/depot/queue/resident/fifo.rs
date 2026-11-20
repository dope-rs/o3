//! Bounded payloads with owner-bound lanes and linear FIFO registrations.

use std::{convert::Infallible, marker, mem, num};

use crate::depot::queue::resident;

/// Bounded FIFO value slots with a constructor-proved lane extent.
pub type Fifo<W, L = num::NonZero<usize>> =
    resident::Lanes<resident::raw::Entry<W>, resident::Plain, L>;

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
pub type Registration<'target, W, L = num::NonZero<usize>> = resident::Lease<'target, Fifo<W, L>>;

pub type Reject = resident::Reject<Infallible>;

impl<W, L> Fifo<W, L> {
    /// Borrows lane zero, retained by every constructor's nonzero extent.
    pub fn first(&self) -> resident::Lane<'_, Self> {
        self.at(resident::Position::new::<1, 0>())
    }

    pub fn take_next(&self) -> Option<W> {
        if self.is_draining() {
            return None;
        }
        let entry = self.take::<true>()?;
        // SAFETY: the removed linked entry was initialized by try_keep and
        // remains reserved by its registration while this value is consumed.
        Some(unsafe { entry.take() })
    }
}

impl<W> Fifo<W> {
    pub fn try_lane(
        &self,
        index: usize,
    ) -> Result<resident::Lane<'_, Self>, resident::Bounds<num::NonZero<u32>>> {
        Ok(self.at(self.try_at(index)?))
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
            position: self.try_at(index)?.index().position(),
            brand: marker::PhantomData,
        })
    }

    /// Borrows a lane admitted by this scope's only FIFO.
    pub fn lane(&self, index: Index<'scope>) -> resident::Lane<'_, Self> {
        self.at(resident::Position::from_pos(index.position))
    }
}

impl<W, L> Registration<'_, W, L> {
    pub fn is_active(&self) -> bool {
        self.is_bound()
    }

    /// Retires this slot. If its value was taken, returns the next value in its lane.
    /// During draining, retires without taking a successor.
    pub fn cancel(&mut self) -> Option<W> {
        let (fifo, key) = self.active()?;
        let key = *key;
        self.detach();
        let lane = fifo.remove(key)?;
        if fifo.is_draining() {
            return None;
        }
        fifo.at(lane).take_one()
    }
}

impl<W, L> resident::Target for Fifo<W, L> {
    type Parked = Infallible;

    fn retire(&self, key: resident::Key) {
        let _ = self.remove(key);
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

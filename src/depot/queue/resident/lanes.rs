use std::{marker, mem, num};

use crate::{
    alloc,
    base::marker::ThreadBound,
    depot::{
        self,
        queue::{
            links,
            resident::{self, Bounds, Capacity, Error, Extent},
            round,
        },
    },
};

/// One resident entry keyed by its exact index and current lane.
#[derive(Clone, Copy)]
pub(super) struct Key {
    index: links::Index,
    lane: Lane,
}

/// One lane admitted by its owner's immutable extent.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(transparent)]
pub(super) struct Lane(links::Index);

impl Lane {
    pub(super) fn new<const N: usize, const I: usize>() -> Self {
        const { assert!(I < N && N <= u32::MAX as usize) };
        Self(links::Index::from(
            num::NonZero::<u32>::MIN.saturating_add(I as u32),
        ))
    }

    pub(super) const fn index(self) -> links::Index {
        self.0
    }

    pub(super) const fn from_pos(position: num::NonZero<u32>) -> Self {
        Self(links::Index::from_pos(position))
    }
}

/// Marker for lanes whose residents leave through `pop` or `take`.
pub(super) enum Plain {}

/// Marker for lanes whose residents may park awaiting an explicit claim.
pub(super) enum Parking {}

/// Stable resident values partitioned among runtime FIFO lanes and an active ring.
pub(super) struct Lanes<T, M, L = num::NonZero<usize>> {
    storage: alloc::raw::Block<Capacity<T, L>, (), (), resident::raw::Node<T>>,
    thread: marker::PhantomData<(ThreadBound, M)>,
    round: links::Cycle,
    free: links::Free,
}

/// Exact outcome of removing one resident from a parking lane.
pub(super) enum Removal<'a, T> {
    Linked,
    Parked(&'a T),
    Empty,
}

impl<T, M, L> Lanes<T, M, L> {
    pub(super) fn try_new(capacity: usize, lanes: L) -> Result<Self, Error>
    where
        T: Default,
        L: Extent,
    {
        let extent = Capacity::new(capacity, lanes)?;
        let capacity = alloc::raw::Shape::len(&extent);
        let storage = alloc::raw::Block::try_from_fn(
            extent,
            (),
            |index| {
                let next = index + 1;
                let next = if capacity > next {
                    next as u32
                } else {
                    u32::MAX
                };
                resident::raw::Node::vacant(T::default(), index as u32, next)
            },
            |index| resident::raw::Node::vacant(links::Chain::new(), index as u32, u32::MAX),
        )
        .map_err(Error::Exhausted)?;
        Ok(Self {
            storage,
            thread: marker::PhantomData,
            round: links::Cycle::new(),
            free: links::Free::seeded(capacity),
        })
    }

    pub(super) fn try_lane(&self, index: usize) -> Result<Lane, Bounds<num::NonZero<u32>>> {
        let lanes = self.storage.extent().lanes();
        if index >= lanes.get() as usize {
            return Err(Bounds::new(index, lanes));
        }
        // SAFETY: this original u32 lane extent proves the tail bound and excludes END.
        Ok(Lane(unsafe { links::Index::new_usize(index) }))
    }

    pub(super) fn try_push<E: Copy>(&self, lane: Lane, full: E) -> Result<(Key, &T), E> {
        let entries = self.storage.as_ref();
        // SAFETY: `self.free` and `entries` share this one `Capacity` extent, and
        // every hooked key was acquired from that same slice, so the vacant chain
        // stays acyclic inside it.
        let Some(view) = (unsafe { resident::raw::View::acquire(&self.free, entries) }) else {
            return Err(full);
        };
        // SAFETY: acquisition retains this detached hook and the admitted lane
        // belongs to this exact owner's reciprocal active ring.
        round::Join::insert(unsafe {
            depot::raw::Proof::new((self.join(lane, entries), *view.key()))
        });
        Ok((
            Key {
                index: *view.key(),
                lane,
            },
            view.into_ref(),
        ))
    }

    pub(super) fn relink_back(&self, key: Key, lane: Lane) -> (Key, &T) {
        let entries = self.storage.as_ref();
        let view = Self::view(entries, key.index);
        let target = self.join(lane, entries);
        if is_linked(view) {
            if key.lane == lane {
                return (key, view.into_ref());
            }
            // SAFETY: this key retains its exact current lane membership.
            let vacant = round::Join::remove(unsafe {
                depot::raw::Proof::new((self.join(key.lane, entries), key.index))
            });
            // SAFETY: removal transfers the same hook into its admitted destination.
            round::Join::push(unsafe { depot::raw::Proof::new((target, vacant)) });
        } else {
            // SAFETY: the retained key is detached in this exact owner's store.
            round::Join::insert(unsafe { depot::raw::Proof::new((target, key.index)) });
        }
        (
            Key {
                index: key.index,
                lane,
            },
            view.into_ref(),
        )
    }

    pub(super) fn pop(&self, lane: Lane) -> Option<resident::raw::View<'_, T>> {
        let entries = self.storage.as_ref();
        // SAFETY: the admitted lane retains its reciprocal FIFO and exact active ring.
        let vacant = round::Join::pop(unsafe { depot::raw::Proof::new(self.join(lane, entries)) })?;
        let view = Self::view(entries, vacant.index());
        vacant.idle();
        Some(view)
    }

    pub(super) fn take<const ROTATE: bool>(&self) -> Option<&T> {
        let entries = self.storage.as_ref();
        let lane = Lane(self.round.first()?);
        // SAFETY: this exact active front proves a reciprocal nonempty lane.
        let vacant = round::Join::active::<ROTATE>(unsafe {
            depot::raw::Proof::new(self.join(lane, entries))
        });
        let view = Self::view(entries, vacant.index());
        vacant.idle();
        Some(view.into_ref())
    }

    pub(super) fn has_waiters(&self) -> bool {
        !self.round.is_empty()
    }

    pub(super) fn is_empty(&self, lane: Lane) -> bool {
        let lanes = self.storage.tail();
        // SAFETY: the admitted lane belongs to this owner's immutable lane extent.
        unsafe { resident::raw::View::new(lanes, lane.index()) }.is_empty()
    }

    fn join<'a>(
        &'a self,
        lane: Lane,
        entries: &'a [resident::raw::Node<T>],
    ) -> round::Join<'a, resident::raw::Slice<T>, resident::raw::Slice<links::Chain>> {
        let lanes = self.storage.tail();
        // SAFETY: each private Lane was admitted by this owner's immutable extent.
        let lane = unsafe { resident::raw::View::new(lanes, lane.index()) };
        round::Join {
            hooks: resident::raw::Slice::new(entries),
            lanes: resident::raw::Slice::new(lanes),
            entries: lane.into_ref(),
            active: &self.round,
            index: *lane.key(),
        }
    }

    fn view(entries: &[resident::raw::Node<T>], index: links::Index) -> resident::raw::View<'_, T> {
        // SAFETY: callers retain a key or hook admitted by this exact owned store.
        unsafe { resident::raw::View::new(entries, index) }
    }
}

impl<T, L> Lanes<T, Parking, L> {
    pub(super) fn park(&self, lane: Lane) -> Option<&T> {
        let entries = self.storage.as_ref();
        // SAFETY: the admitted lane retains its reciprocal FIFO and exact active ring.
        let vacant = round::Join::pop(unsafe { depot::raw::Proof::new(self.join(lane, entries)) })?;
        let view = Self::view(entries, vacant.index());
        vacant.park();
        Some(view.into_ref())
    }

    pub(super) fn is_parked(&self, key: Key) -> bool {
        Self::view(self.storage.as_ref(), key.index).is_parked()
    }

    pub(super) fn claim(&self, key: Key) -> Option<&T> {
        let view = Self::view(self.storage.as_ref(), key.index);
        let vacant = view.unpark()?;
        self.free.recycle(vacant);
        Some(&**view.into_ref())
    }

    pub(super) fn remove(&self, key: Key) -> Removal<'_, T> {
        let entries = self.storage.as_ref();
        let view = Self::view(entries, key.index);
        if is_linked(view) {
            // SAFETY: this key retains its exact current lane membership.
            let vacant = round::Join::remove(unsafe {
                depot::raw::Proof::new((self.join(key.lane, entries), key.index))
            });
            self.free.recycle(vacant);
            Removal::Linked
        } else if let Some(vacant) = view.unpark() {
            self.free.recycle(vacant);
            Removal::Parked(&**view.into_ref())
        } else {
            // SAFETY: this key is neither linked nor parked, so its hook is
            // detached from every list of this owner and returns to `self.free`,
            // which owns this exact slice.
            unsafe {
                depot::raw::Hooks::recycle(
                    resident::raw::Slice::new(entries),
                    &self.free,
                    *view.key(),
                )
            };
            Removal::Empty
        }
    }
}

impl<T, L> Lanes<T, Plain, L> {
    pub(super) fn remove(&self, key: Key) {
        let entries = self.storage.as_ref();
        let view = Self::view(entries, key.index);
        if is_linked(view) {
            // SAFETY: this key retains its exact current lane membership.
            let vacant = round::Join::remove(unsafe {
                depot::raw::Proof::new((self.join(key.lane, entries), key.index))
            });
            self.free.recycle(vacant);
        } else {
            // SAFETY: this key is not linked, so its hook is detached from every
            // list of this owner and returns to `self.free`, which owns this exact
            // slice.
            unsafe {
                depot::raw::Hooks::recycle(
                    resident::raw::Slice::new(entries),
                    &self.free,
                    *view.key(),
                )
            };
        }
    }
}

fn is_linked<T>(view: resident::raw::View<'_, T>) -> bool {
    view.key().is_linked(view.hook())
}

const _: () = {
    assert!(
        mem::size_of::<alloc::raw::Block<Capacity<()>, (), (), resident::raw::Node<()>>>() == 24
    );
    assert!(mem::size_of::<Lanes<(), Plain>>() == 4 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Lanes<(), Plain>>>() == mem::size_of::<Lanes<(), Plain>>());
};

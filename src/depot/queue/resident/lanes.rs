use std::{marker, mem, num, ptr};

use crate::{
    alloc,
    base::marker::ThreadBound,
    depot::{
        self,
        queue::{
            links,
            resident::{self, Bounds, Capacity, Draining, Error, Extent, Target, raw::Entry},
            round,
        },
    },
};

/// One resident entry keyed by its exact index and current lane.
#[derive(Clone, Copy)]
pub struct Key {
    index: links::Index,
    lane: Position,
}

/// One lane admitted by its owner's immutable extent.
#[derive(Clone, Copy, Eq, PartialEq)]
#[repr(transparent)]
pub(super) struct Position(links::Index);

impl Position {
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

/// A lane borrowed from the store that admitted its position.
pub struct Lane<'a, T> {
    pub(super) owner: &'a T,
    pub(super) position: Position,
}

impl<T> Clone for Lane<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Lane<'_, T> {}

/// Marker for lanes whose residents leave through `pop` or `take`.
pub enum Plain {}

/// Marker for lanes whose residents may park awaiting an explicit claim.
pub enum Parking {}

/// Stable resident values partitioned among runtime FIFO lanes and an active ring.
pub struct Lanes<T, M, L = num::NonZero<usize>> {
    storage: alloc::raw::Block<Capacity<T, L>, (), (), resident::raw::Node<T>>,
    thread: marker::PhantomData<(ThreadBound, M)>,
    round: links::Cycle,
    free: links::Free,
    pub(super) draining: Draining,
}

/// Exact outcome of removing one resident from a parking lane.
pub(super) enum Removal<'a, T> {
    Linked,
    Parked(&'a T),
    Empty,
}

impl<T, M, L> Lanes<T, M, L> {
    pub fn try_new(capacity: usize, lanes: L) -> Result<Self, Error>
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
            draining: Draining::default(),
        })
    }

    pub(super) fn at(&self, position: Position) -> Lane<'_, Self> {
        Lane {
            owner: self,
            position,
        }
    }

    pub(super) fn try_at(&self, index: usize) -> Result<Position, Bounds<num::NonZero<u32>>> {
        let lanes = self.storage.extent().lanes();
        if index >= lanes.get() as usize {
            return Err(Bounds::new(index, lanes));
        }
        // SAFETY: this original u32 lane extent proves the tail bound and excludes END.
        Ok(Position(unsafe { links::Index::new_usize(index) }))
    }

    fn insert(&self, lane: Position) -> Option<(Key, &T)> {
        let entries = self.storage.as_ref();
        // SAFETY: `self.free` and `entries` share this one `Capacity` extent, and
        // every hooked key was acquired from that same slice, so the vacant chain
        // stays acyclic inside it.
        let view = unsafe { resident::raw::View::acquire(&self.free, entries) }?;
        // SAFETY: acquisition retains this detached hook and the admitted lane
        // belongs to this exact owner's reciprocal active ring.
        round::Join::insert(unsafe {
            depot::raw::Proof::new((self.join(lane, entries), *view.key()))
        });
        Some((
            Key {
                index: *view.key(),
                lane,
            },
            view.into_ref(),
        ))
    }

    fn relink_back(&self, key: &mut Key, lane: Position, linked: bool) {
        let entries = self.storage.as_ref();
        let target = self.join(lane, entries);
        if linked {
            if key.lane == lane {
                return;
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
        key.lane = lane;
    }

    pub(super) fn pop(&self, lane: Position) -> Option<resident::raw::View<'_, T>> {
        let entries = self.storage.as_ref();
        // SAFETY: the admitted lane retains its reciprocal FIFO and exact active ring.
        let vacant = round::Join::pop(unsafe { depot::raw::Proof::new(self.join(lane, entries)) })?;
        let view = Self::view(entries, vacant.index());
        vacant.idle();
        Some(view)
    }

    pub(super) fn take<const ROTATE: bool>(&self) -> Option<&T> {
        let entries = self.storage.as_ref();
        let lane = Position(self.round.first()?);
        // SAFETY: this exact active front proves a reciprocal nonempty lane.
        let vacant = round::Join::active::<ROTATE>(unsafe {
            depot::raw::Proof::new(self.join(lane, entries))
        });
        let view = Self::view(entries, vacant.index());
        vacant.idle();
        Some(view.into_ref())
    }

    pub fn has_waiters(&self) -> bool {
        !self.round.is_empty()
    }

    pub fn begin_drain(&self) {
        self.draining.begin();
    }

    pub fn is_draining(&self) -> bool {
        self.draining.is_active()
    }

    pub(super) fn is_empty(&self, lane: Position) -> bool {
        let lanes = self.storage.tail();
        // SAFETY: the admitted lane belongs to this owner's immutable lane extent.
        unsafe { resident::raw::View::new(lanes, lane.index()) }.is_empty()
    }

    fn join<'a>(
        &'a self,
        lane: Position,
        entries: &'a [resident::raw::Node<T>],
    ) -> round::Join<'a, [resident::raw::Node<T>], [resident::raw::Node<links::Chain>]> {
        let lanes = self.storage.tail();
        // SAFETY: each private Position was admitted by this owner's immutable extent.
        let lane = unsafe { resident::raw::View::new(lanes, lane.index()) };
        round::Join {
            hooks: entries,
            lanes,
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

impl<T, M, const N: usize> Lanes<T, M, resident::Fixed<N>> {
    pub fn lane<const I: usize>(&self) -> Lane<'_, Self> {
        self.at(Position::new::<N, I>())
    }
}

impl<W, G, M, L> Lanes<Entry<W, G>, M, L> {
    /// Takes the next value in drain order and ends draining at quiescence.
    #[must_use = "a drained item remains caller-owned"]
    pub fn drain_next(&self) -> Option<W> {
        self.draining.drain(|| {
            let entry = self.take::<false>()?;
            // SAFETY: take removed one initialized wake value from the ring.
            Some(unsafe { entry.take() })
        })
    }
}

impl<'a, W, G, M, L> Lane<'a, Lanes<Entry<W, G>, M, L>> {
    pub fn try_keep<P>(
        self,
        slot: &mut resident::Lease<'a, Lanes<Entry<W, G>, M, L>>,
        value: W,
    ) -> Result<(), resident::Reject<P>>
    where
        W: Copy,
        Lanes<Entry<W, G>, M, L>: Target<Parked = P>,
    {
        let index = self.position.index().get();
        if self.owner.is_draining() {
            return Err(resident::Reject::Draining { index });
        }
        let entry = match slot.active() {
            Some((active, key)) => {
                let view = Lanes::<_, M, L>::view(active.storage.as_ref(), key.index);
                let linked = is_linked(view);
                if !linked
                    && let Some(index) =
                        <Lanes<Entry<W, G>, M, L> as Target>::detached_grant(view, index)
                {
                    return Err(resident::Reject::GrantPending { index });
                }
                if !ptr::eq(active, self.owner) {
                    return Err(resident::Reject::Active { index });
                }
                active.relink_back(key, self.position, linked);
                &**view.into_ref()
            }
            None => {
                let Some((key, entry)) = self.owner.insert(self.position) else {
                    return Err(resident::Reject::Full { index });
                };
                slot.set(self.owner, key);
                entry
            }
        };
        entry.initialize(value);
        Ok(())
    }

    #[must_use = "a taken item remains caller-owned"]
    pub fn take_one(self) -> Option<W> {
        let entry = self.owner.pop(self.position)?;
        // SAFETY: pop detached one initialized wake value; its registration retains the slot.
        Some(unsafe { entry.take() })
    }
}

impl<T, L> Lanes<T, Parking, L> {
    pub(super) fn park(&self, lane: Position) -> Option<&T> {
        let entries = self.storage.as_ref();
        // SAFETY: the admitted lane retains its reciprocal FIFO and exact active ring.
        let vacant = round::Join::pop(unsafe { depot::raw::Proof::new(self.join(lane, entries)) })?;
        let view = Self::view(entries, vacant.index());
        vacant.park();
        Some(view.into_ref())
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
            unsafe { depot::raw::Hooks::recycle(entries, &self.free, *view.key()) };
            Removal::Empty
        }
    }
}

impl<T, L> Lanes<T, Plain, L> {
    pub(super) fn remove(&self, key: Key) -> Option<Position> {
        let entries = self.storage.as_ref();
        let view = Self::view(entries, key.index);
        if is_linked(view) {
            // SAFETY: this key retains its exact current lane membership.
            let vacant = round::Join::remove(unsafe {
                depot::raw::Proof::new((self.join(key.lane, entries), key.index))
            });
            self.free.recycle(vacant);
            None
        } else {
            // SAFETY: this key is not linked, so its hook is detached from every
            // list of this owner and returns to `self.free`, which owns this exact
            // slice.
            unsafe { depot::raw::Hooks::recycle(entries, &self.free, *view.key()) };
            Some(key.lane)
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
    assert!(mem::size_of::<Lanes<(), Plain>>() == 5 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Lanes<(), Plain>>>() == mem::size_of::<Lanes<(), Plain>>());
};

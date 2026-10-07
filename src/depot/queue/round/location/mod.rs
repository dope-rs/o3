use std::{cell, marker, mem, ops};

use crate::depot::queue::{
    links,
    round::{self, Conflict, Error, Idle, Linked, lanes::stores, phase},
};

pub(super) mod conflict;
pub(super) mod error;
pub(super) mod idle;
pub(super) mod linked;

/// A position retaining its exact owner and admitted membership phase.
#[derive(Clone, Copy, Debug)]
pub struct Location<O, P = ()> {
    pub(in crate::depot::queue::round) owner: O,
    pub(in crate::depot::queue::round) position: round::Position,
    phase: marker::PhantomData<P>,
}

pub(in crate::depot::queue::round) struct Access<'a, T> {
    node: &'a stores::Node<T>,
    pub(in crate::depot::queue::round) track: &'a stores::Track,
    ownership: &'a cell::Cell<links::Index>,
    slots: &'a [stores::Node<T>],
    tracks: &'a [stores::Track],
    pub(in crate::depot::queue::round) group: &'a links::Cycle,
}

impl<'a, T> Access<'a, T> {
    fn lane(&self, index: links::Index) -> round::Join<'a, [stores::Node<T>], [stores::Track]> {
        round::Join {
            hooks: self.slots,
            lanes: self.tracks,
            entries: &self.track.entries,
            active: self.group,
            index,
        }
    }

    pub(in crate::depot::queue::round) fn linked_lane(
        &self,
        index: links::Index,
    ) -> Option<links::Index> {
        index
            .is_linked(&self.node.hook)
            .then_some(self.ownership.get())
    }
}

impl<O, P> Location<O, P> {
    pub(in crate::depot::queue::round) fn new(owner: O, position: round::Position) -> Self {
        Self {
            owner,
            position,
            phase: marker::PhantomData,
        }
    }
    pub(in crate::depot::queue::round) fn phase<Q>(self) -> Location<O, Q> {
        Location::new(self.owner, self.position)
    }
    pub fn index(&self) -> usize {
        self.position.index.get()
    }
    pub const fn lane(&self) -> usize {
        self.position.lane.get()
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>, P>
    Location<O, P>
{
    pub(in crate::depot::queue::round) fn access(&self) -> Access<'_, T> {
        let owner = &*self.owner;
        // SAFETY: the location owns the borrow against which both indices were admitted.
        Access {
            node: unsafe {
                owner
                    .block
                    .as_ref()
                    .get_unchecked(self.position.index.get())
            },
            track: unsafe { owner.block.tail().get_unchecked(self.position.lane.get()) },
            ownership: unsafe {
                owner
                    .block
                    .values()
                    .get_unchecked(self.position.index.get())
            },
            slots: owner.block.as_ref(),
            tracks: owner.block.tail(),
            group: &owner.groups[self.position.lane.get() % G],
        }
    }

    pub(in crate::depot::queue::round) fn push_from(mut self, lane: Option<links::Index>) {
        let target = self.position.lane;
        let vacant = if let Some(lane) = lane {
            self.position.lane = lane;
            let mut entry = self.phase::<Linked>().remove();
            entry.position.lane = target;
            entry.phase::<Idle<true>>()
        } else {
            self.phase::<Idle<true>>()
        };
        vacant.push_back();
    }
}

impl<'a, T, const G: usize, const N: usize, P> Location<&'a round::Lanes<T, G, N>, P> {
    pub fn get(&self) -> &'a T {
        // SAFETY: the retained owner admits this resident for the full source lifetime.
        unsafe {
            &self
                .owner
                .block
                .as_ref()
                .get_unchecked(self.position.index.get())
                .value
        }
    }
}

impl<'a, T, const G: usize, const N: usize> Location<&'a mut round::Lanes<T, G, N>> {
    pub fn try_insert(
        self,
    ) -> Result<
        Location<&'a mut round::Lanes<T, G, N>, Linked>,
        Conflict<&'a mut round::Lanes<T, G, N>>,
    > {
        let access = self.access();
        if !self.position.index.is_linked(&access.node.hook) {
            return Ok(self.phase::<Idle<true>>().push_back());
        }
        let actual = access.ownership.get();
        let target = self.position.lane;
        let mut entry = self;
        entry.position.lane = actual;
        let member = entry.phase::<Linked>();
        if actual != target {
            return Err(Conflict {
                member,
                requested: target,
            });
        }
        Ok(member)
    }

    pub fn try_remove(
        self,
    ) -> Result<
        Location<&'a mut round::Lanes<T, G, N>, Idle<false>>,
        Error<&'a mut round::Lanes<T, G, N>>,
    > {
        let access = self.access();
        if !self.position.index.is_linked(&access.node.hook) {
            return Err(Error::Vacant(self.phase()));
        }
        let actual = access.ownership.get();
        let target = self.position.lane;
        let mut entry = self;
        entry.position.lane = actual;
        let member = entry.phase::<Linked>();
        if actual != target {
            return Err(Error::Lane(Conflict {
                member,
                requested: target,
            }));
        }
        Ok(member.remove())
    }
}

impl<T, const G: usize, const N: usize, P> Location<&mut round::Lanes<T, G, N>, P> {
    /// Borrows the admitted resident independently of its membership phase.
    pub fn get(&self) -> &T {
        &self.access().node.value
    }
}

impl<
    T,
    const G: usize,
    const N: usize,
    O: ops::Deref<Target = round::Lanes<T, G, N>>,
    const ASSIGN: bool,
> Location<O, Idle<ASSIGN>>
{
    pub fn push_back(self) -> Location<O, Linked> {
        let entry = self;
        let access = entry.access();
        // SAFETY: this consuming transition owns the exact detached resident and owner.
        round::Join::insert(unsafe {
            crate::depot::raw::Proof::new((access.lane(entry.position.lane), entry.position.index))
        });
        if ASSIGN {
            access.ownership.set(entry.position.lane);
        }
        entry.phase()
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>>
    Location<O, Linked>
{
    pub fn remove(self) -> Location<O, Idle<false>> {
        let entry = self;
        let access = entry.access();
        // SAFETY: Member owns this exact membership until this transition consumes it.
        round::Join::remove(unsafe {
            crate::depot::raw::Proof::new((access.lane(entry.position.lane), entry.position.index))
        })
        .idle();
        entry.phase()
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>>
    Location<O, phase::Head>
{
    pub(in crate::depot::queue::round) fn remove<const ROTATE: bool>(
        self,
    ) -> Location<O, Idle<false>> {
        let entry = self;
        let access = entry.access();
        // SAFETY: Head retains the exact FIFO front until this consuming transition.
        let source = unsafe {
            crate::depot::raw::Proof::new((access.lane(entry.position.lane), entry.position.index))
        };
        round::Join::head::<ROTATE>(source).idle();
        entry.phase()
    }
}

impl<'a, T, const G: usize, const N: usize> Location<&'a round::Lanes<T, G, N>, phase::Head> {
    pub fn pop(self) -> &'a T {
        self.remove::<false>().get()
    }

    /// Advances this nonempty lane while retaining its unchanged topology authority.
    pub fn rotate(self) -> Self {
        let mut entry = self.remove::<true>().push_back();
        // SAFETY: this linked member proves that its exact lane remains nonempty.
        entry.position.index = unsafe { entry.access().track.entries.front_key() };
        entry.phase()
    }

    pub(in crate::depot::queue::round) fn take_active(
        source: crate::depot::raw::Proof<Self, phase::Active>,
    ) -> &'a T {
        let entry = source.into_inner();
        let access = entry.access();
        // SAFETY: the source retains this exact active-ring front and its
        // nonempty FIFO without callbacks between admission and removal.
        round::Join::active::<true>(unsafe {
            crate::depot::raw::Proof::new(access.lane(entry.position.lane))
        })
        .idle();
        entry.get()
    }
}

const _: () = {
    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<Location<&'static mut round::Lanes<(), 1>>>() == 2 * word);
    assert!(mem::size_of::<Location<&'static mut round::Lanes<(), 1>, Linked>>() == 2 * word);
    assert!(
        mem::size_of::<Option<Location<&'static mut round::Lanes<(), 1>, Idle<false>>>>()
            == 2 * word
    );
    assert!(mem::size_of::<Conflict<&'static mut round::Lanes<(), 1>>>() == 3 * word);
};

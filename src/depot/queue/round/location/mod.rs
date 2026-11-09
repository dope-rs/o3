use std::{marker, mem, ops};

use crate::depot::queue::round::{self, Idle, Linked, phase};

pub(super) mod idle;
pub(super) mod linked;

/// A position retaining its exact owner and admitted membership phase.
#[derive(Clone, Copy, Debug)]
pub struct Location<O, P = ()> {
    pub(in crate::depot::queue::round) owner: O,
    pub(in crate::depot::queue::round) position: round::Position,
    phase: marker::PhantomData<P>,
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

impl<'a, T, const G: usize, const N: usize, P> Location<&'a round::Lanes<T, G, N>, P> {
    pub fn get(&self) -> &'a T {
        round::Slot {
            owner: self.owner,
            index: self.position.index,
        }
        .get()
    }
}

impl<T, const G: usize, const N: usize, P> Location<&mut round::Lanes<T, G, N>, P> {
    /// Borrows the admitted resident independently of its membership phase.
    pub fn get(&self) -> &T {
        round::Slot {
            owner: &*self.owner,
            index: self.position.index,
        }
        .get()
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>>
    Location<O, Idle>
{
    pub fn push_back(self) -> Location<O, Linked> {
        let owner = &*self.owner;
        // SAFETY: this location retains its admitted lane and exact detached resident.
        round::Join::insert(unsafe {
            crate::depot::raw::Proof::new((owner.join(self.position.lane), self.position.index))
        });
        self.phase()
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>>
    Location<O, Linked>
{
    pub fn remove(self) -> Location<O, Idle> {
        let owner = &*self.owner;
        // SAFETY: this location retains its admitted lane and exact linked membership.
        round::Join::remove(unsafe {
            crate::depot::raw::Proof::new((owner.join(self.position.lane), self.position.index))
        })
        .idle();
        self.phase()
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>>
    Location<O, phase::Head>
{
    pub(in crate::depot::queue::round) fn remove<const ROTATE: bool>(self) -> Location<O, Idle> {
        let owner = &*self.owner;
        // SAFETY: Head retains this owner's admitted lane and exact FIFO front.
        let source = unsafe {
            crate::depot::raw::Proof::new((owner.join(self.position.lane), self.position.index))
        };
        round::Join::head::<ROTATE>(source).idle();
        self.phase()
    }
}

impl<'a, T, const G: usize, const N: usize> Location<&'a round::Lanes<T, G, N>, phase::Head> {
    pub fn pop(self) -> &'a T {
        self.remove::<false>().get()
    }

    /// Advances this nonempty lane while retaining its unchanged topology authority.
    pub fn rotate(mut self) -> Self {
        // SAFETY: Head retains this lane's admission to the unchanged owner.
        let lane = unsafe { self.owner.join(self.position.lane) };
        let entries = lane.entries;
        // SAFETY: Head retains this exact FIFO front without an intervening callback.
        let vacant = round::Join::head::<true>(unsafe {
            crate::depot::raw::Proof::new((round::Join { ..lane }, self.position.index))
        });
        // SAFETY: this consuming transfer retains the same detached hook and admitted lane.
        round::Join::push(unsafe { crate::depot::raw::Proof::new((lane, vacant)) });
        // SAFETY: reinsertion leaves this exact lane nonempty.
        self.position.index = unsafe { entries.front_key() };
        self
    }

    pub(in crate::depot::queue::round) fn take_active(
        source: crate::depot::raw::Proof<Self, phase::Active>,
    ) -> &'a T {
        let entry = source.into_inner();
        // SAFETY: the source retains this admitted lane and both exact fronts
        // without callbacks between admission and removal.
        let lane = unsafe { entry.owner.join(entry.position.lane) };
        unsafe { lane.remove_front::<true, true>(entry.position.index) }.idle();
        entry.get()
    }
}

const _: () = {
    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<Location<&'static mut round::Lanes<(), 1>>>() == 2 * word);
    assert!(mem::size_of::<Location<&'static mut round::Lanes<(), 1>, Linked>>() == 2 * word);
    assert!(mem::size_of::<Option<Location<&'static mut round::Lanes<(), 1>, Idle>>>() == 2 * word);
};

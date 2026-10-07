use std::{marker, mem, ops};

use crate::depot::{
    self,
    queue::{
        links,
        round::{self, Lanes, phase},
    },
};

/// One lane resolved against its exact owner.
#[derive(Clone, Copy)]
pub struct Lane<O> {
    pub(in crate::depot::queue::round) owner: O,
    pub(in crate::depot::queue::round) index: links::Index,
    pub(super) item: marker::PhantomData<fn(O) -> O>,
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>> Lane<O> {
    /// Admits one lane and retains the supplied shared or exclusive owner borrow.
    pub fn try_new(owner: O, index: usize) -> Result<Self, round::Bounds> {
        let capacity = owner.block.tail().len();
        if index >= capacity {
            return Err(round::Bounds::Lane { index, capacity });
        }
        Ok(Self {
            owner,
            index: unsafe { links::Index::new_usize(index) },
            item: marker::PhantomData,
        })
    }
}

impl<T, const G: usize, const N: usize, O: ops::Deref<Target = round::Lanes<T, G, N>>>
    depot::raw::Admit for depot::raw::Proof<Lane<O>, phase::Ready>
{
    type Output = Option<round::Location<O, phase::Head>>;

    fn admit(self) -> Self::Output {
        let Lane {
            owner, index: lane, ..
        } = self.into_inner();
        // SAFETY: the proof retains the owner against which the lane was admitted.
        let track = unsafe { owner.block.tail().get_unchecked(lane.get()) };
        let index = track.entries.first()?;
        Some(round::Location::new(owner, round::Position { lane, index }))
    }
}

impl<T, const G: usize, const N: usize> Lane<&mut round::Lanes<T, G, N>> {
    pub fn dequeue(
        &mut self,
    ) -> Option<round::Location<&mut round::Lanes<T, G, N>, round::Idle<false>>> {
        let lane = Lane {
            owner: &mut *self.owner,
            index: self.index,
            item: marker::PhantomData,
        };
        // SAFETY: this exclusive lane borrow retains its topology through immediate removal.
        Some(
            depot::raw::Admit::admit(unsafe { depot::raw::Proof::<_, phase::Ready>::new(lane) })?
                .remove::<true>(),
        )
    }
}

impl<'a, T, const G: usize, const N: usize> Lane<&'a round::Lanes<T, G, N>> {
    pub fn front(&self) -> Option<round::Snapshot<&'a round::Lanes<T, G, N>, round::Front>> {
        // SAFETY: the admitted front is immediately converted to a revocable snapshot.
        let head =
            depot::raw::Admit::admit(unsafe { depot::raw::Proof::<_, phase::Ready>::new(*self) })?;
        Some(round::Snapshot::new(head.owner, head.position))
    }

    pub fn pop_front(&self) -> Option<&'a T> {
        // SAFETY: the selected front is consumed without an intervening callback.
        Some(
            depot::raw::Admit::admit(unsafe { depot::raw::Proof::<_, phase::Ready>::new(*self) })?
                .pop(),
        )
    }

    /// Advances this lane's front and retains the resulting front snapshot.
    pub fn rotate(&self) -> Option<round::Snapshot<&'a round::Lanes<T, G, N>, round::Front>> {
        // SAFETY: the selected front is rotated without an intervening callback.
        let head =
            depot::raw::Admit::admit(unsafe { depot::raw::Proof::<_, phase::Ready>::new(*self) })?
                .rotate();
        Some(round::Snapshot::new(head.owner, head.position))
    }
}

const _: () = {
    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<Lane<&Lanes<(), 1>>>() == 2 * word);
    assert!(mem::size_of::<Option<Lane<&Lanes<(), 1>>>>() == 2 * word);
    assert!(mem::size_of::<Lane<&mut Lanes<(), 1>>>() == 2 * word);
    assert!(mem::size_of::<Option<Lane<&mut Lanes<(), 1>>>>() == 2 * word);
};

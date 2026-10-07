pub(in crate::depot) use block::QueueBlock;

use crate::depot::arena::linked::slots;

mod block;

use crate::depot::queue::{indexed::shared, pool};

pub(in crate::depot) struct QueueSlot<S> {
    pub(in crate::depot) queue: slots::State,
    pub(in crate::depot) state: S,
}

// SAFETY: Live retains this exact allocated, initialized lane and the owner's borrow.
unsafe impl<'a, T, S, const TRACK: bool> crate::depot::raw::Project<usize>
    for &'a pool::Lanes<T, S, TRACK>
{
    type Output = pool::View<'a, T, S, TRACK>;

    unsafe fn project(self, index: usize) -> Self::Output {
        pool::View::new(&self.block.store, unsafe {
            self.block.slots().get_unchecked(index)
        })
    }
}

// SAFETY: Live retains the exclusive original owner and this initialized lane.
unsafe impl<'a, T, S, const TRACK: bool> crate::depot::raw::Project<usize>
    for &'a mut pool::Lanes<T, S, TRACK>
{
    type Output = pool::Lane<'a, T, S, TRACK>;

    unsafe fn project(self, index: usize) -> Self::Output {
        let (store, slots) = self.block.split();
        pool::Lane::new(store, unsafe { slots.get_unchecked_mut(index) })
    }
}

// SAFETY: the initialized-node proof retains this exact allocation and its original borrow.
unsafe impl<'a, T: Copy, P, const N: usize> crate::depot::raw::Project<usize>
    for &'a shared::Fifo<T, P, N>
{
    type Output = shared::Slot<'a, T, P, N>;

    unsafe fn project(self, index: usize) -> Self::Output {
        shared::Slot {
            fifo: self,
            // SAFETY: Project admits an initialized physical node in this exact owner.
            node: unsafe { self.nodes.get_unchecked(index) },
        }
    }
}

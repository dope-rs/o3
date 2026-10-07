use crate::depot::queue::links;
pub(in crate::depot) mod position;
pub(in crate::depot) mod value;

use std::mem;

use lanes::Value;

use crate::{
    alloc::array,
    depot::{
        capacity,
        queue::{
            indexed::lanes::{
                self,
                entry::{Error, State},
            },
            tagged::{self, entry},
        },
    },
};

impl<T, const N: usize, C: crate::depot::raw::Count> Fifo<T, N, C> {
    pub fn try_from(capacity: C) -> Result<Self, crate::alloc::Error> {
        let shape = tagged::Domain::<[links::Chain; N], C>::new(capacity);
        Self::try_new(shape)
    }

    pub fn entry<const L: usize>(
        &mut self,
        index: usize,
    ) -> Result<State<&T, Vacancy<'_, T, N, L, C>>, Error<C>> {
        const { assert!(L < N) };
        use crate::depot::raw::LaneStore as _;

        let capacity = self.capacity();
        if index >= capacity.count().get() {
            return Err(Error { index, capacity });
        }
        // SAFETY: the original store's immutable extent proved this node's bound.
        let index = unsafe { links::Index::new_usize(index) };
        let node = unsafe { self.nodes().get_unchecked(index.get()) };
        if node.hook.is_linked(index) {
            return Ok(State::Occupied(unsafe {
                (*node.value.get()).assume_init_ref()
            }));
        }
        Ok(State::Vacant(unsafe {
            tagged::Vacant::detached(self, index)
        }))
    }

    pub fn vacant<const L: usize>(
        &mut self,
        index: usize,
    ) -> Result<Vacancy<'_, T, N, L, C>, entry::Error<C>> {
        const {
            assert!(L < N);
        }
        let capacity = self.capacity();
        if index >= capacity.count().get() {
            return Err(entry::Error::Index { index, capacity });
        }
        let position = lanes::Position {
            store: self,
            index: unsafe { links::Index::new_usize(index) },
        };
        tagged::Vacant::try_new::<C>(position)
    }
    pub fn pop_front<const L: usize>(&mut self) -> Option<T> {
        tagged::Endpoint::constant::<L>(self)
            .pop_entry()
            .map(|(_, value)| value)
    }
    pub fn remove_if(&mut self, index: usize, test: impl FnOnce(&T) -> bool) -> Option<T> {
        let entry = tagged::Linked::at(self, index)?;
        if !test(entry.get()) {
            return None;
        }
        Some(entry.remove())
    }
}

pub struct Fifo<T, const N: usize, C: crate::depot::raw::Count = capacity::Count> {
    pub(in crate::depot) nodes:
        array::Block<Value<T>, tagged::Domain<[links::Chain; N], C>, [links::Chain; N]>,
}

impl<T, const N: usize, C: crate::depot::raw::Count> Fifo<T, N, C> {
    pub(in crate::depot) fn try_new(
        shape: tagged::Domain<[links::Chain; N], C>,
    ) -> Result<Self, crate::alloc::Error> {
        let nodes = array::Block::try_new(shape, [const { links::Chain::new() }; N])?
            .init(|index| Value::vacant(unsafe { links::Index::new_usize(index) }));
        Ok(Self { nodes })
    }

    pub(in crate::depot) fn capacity(&self) -> C {
        self.nodes.extent().capacity()
    }
}

impl<T, const N: usize, C: crate::depot::raw::Count> Drop for Fifo<T, N, C> {
    fn drop(&mut self) {
        if mem::needs_drop::<T>() {
            tagged::clear(self);
        }
    }
}

const _: () = assert!(mem::size_of::<Fifo<(), 2>>() == 4 * mem::size_of::<usize>());

/// The vacant slot retains its selected static lane and exact owner loan.
type Vacancy<'a, T, const N: usize, const L: usize, C> =
    tagged::Vacant<lanes::Position<'a, T, N, L, C>>;

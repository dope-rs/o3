use std::marker;

use crate::{
    alloc,
    depot::{heap, index::Position, raw::Address},
};

#[repr(C)]
pub struct Owner<K: Address, P: Ord, R: Copy, I: Position, E, B = ()> {
    pub(super) storage: alloc::raw::Block<heap::Capacity<heap::Stored<K, P>, I, R>, u32, E>,
    key: Domain<K, B>,
}

impl<K: Address, P: Ord, R: Copy, I: Position, E, B> Owner<K, P, R, I, E, B> {
    pub(super) fn new(
        storage: alloc::raw::Block<heap::Capacity<heap::Stored<K, P>, I, R>, u32, E>,
    ) -> Self {
        Self {
            storage,
            key: marker::PhantomData,
        }
    }

    pub const fn len(&self) -> usize {
        self.storage.state as usize
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// SAFETY: the prefix and reverse map stay a bijection within this owner's resident domain.
unsafe impl<K: Address, P: Ord, R: Copy, I: Position, E, B> heap::raw::Store
    for Owner<K, P, R, I, E, B>
{
    type Key = K;
    type Priority = P;
    type Position = I;
    type Storage = heap::Capacity<heap::Stored<K, P>, I, R>;
    type Extra = E;

    fn precedes(left: &K, right: &K) -> bool {
        left.index() < right.index()
    }

    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &self.storage
    }

    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra> {
        &mut self.storage
    }
}

impl<K: Address, P: Ord, R: Copy, I: Position, E, B> Drop for Owner<K, P, R, I, E, B> {
    fn drop(&mut self) {
        // SAFETY: Drop retains the terminal owner until its MaybeUninit backing is deallocated.
        unsafe { heap::raw::Store::destroy(self) };
    }
}

type Domain<K, B> = marker::PhantomData<(fn(K) -> K, B)>;

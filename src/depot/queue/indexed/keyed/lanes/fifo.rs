use std::{marker, mem};

use crate::{
    alloc,
    depot::{capacity, queue::links},
};

/// Heads embedded in the owner of one exclusive lane plane.
#[repr(transparent)]
pub struct Fifo<K: crate::depot::raw::Key, const N: usize, P = ()> {
    pub(super) heads: [links::Chain<()>; N],
    invariant: Domain<K, P>,
}

impl<K: crate::depot::raw::Key, const N: usize, P> Fifo<K, N, P> {
    const VALID: () = assert!(N != 0 && N as u64 <= u32::MAX as u64 + 1);

    pub const fn try_new(cap: capacity::Count) -> Result<Self, alloc::Overflow> {
        let () = Self::VALID;
        if cap.raw() as u64 + N as u64 > u32::MAX as u64 + 1 {
            return Err(alloc::Overflow);
        }
        Ok(Self {
            heads: [const { links::Chain::new() }; N],
            invariant: marker::PhantomData,
        })
    }

    pub fn is_empty<const L: usize>(&self) -> bool {
        const { assert!(L < N) };
        self.heads[L].is_empty()
    }
}

pub(super) const fn endpoint(lane: usize) -> u32 {
    u32::MAX - lane as u32
}

const _: () = assert!(
    mem::size_of::<Fifo<crate::depot::arena::slab::key::Handle, 2>>() == 4 * mem::size_of::<u32>()
);

type Domain<K, P> = marker::PhantomData<fn(K, P) -> (K, P)>;

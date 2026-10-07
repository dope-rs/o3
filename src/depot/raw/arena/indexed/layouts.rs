use std::mem;

use crate::{alloc, depot::arena::slab::key};

#[repr(C)]
pub(in crate::depot) struct IndexedState {
    pub(in crate::depot) free: u32,
    pub(in crate::depot) initialized: u32,
    pub(in crate::depot) len: u32,
    pub(in crate::depot) available: u32,
}

#[repr(C)]
pub(in crate::depot) struct IndexedMeta<const MAX: u32, H: crate::depot::raw::IndexedHook> {
    pub(in crate::depot) generation: key::Generation<MAX>,
    pub(in crate::depot) hook: H,
}

type Fields<T, const MAX: u32, H, const N: usize> = alloc::raw::Tail<
    alloc::raw::Tail<
        alloc::raw::Tail<alloc::raw::Layout<IndexedState>, mem::MaybeUninit<T>>,
        IndexedMeta<MAX, H>,
    >,
    [<H as crate::depot::raw::IndexedHook>::Head; N],
>;

impl IndexedState {
    pub(in crate::depot) const fn values<T>() -> usize {
        const {
            assert!(
                mem::size_of::<Self>() as u128 <= alloc::raw::Layout::<mem::MaybeUninit<T>>::MAX,
                "layout"
            );
        };
        // SAFETY: the fixed header fits after rounding to this element alignment.
        unsafe { alloc::raw::Layout::<mem::MaybeUninit<T>>::offset(mem::size_of::<Self>()) }
    }

    pub(in crate::depot) fn recipe<
        T,
        const MAX: u32,
        H: crate::depot::raw::IndexedHook,
        const N: usize,
    >(
        capacity: usize,
    ) -> impl alloc::raw::Recipe<Fields = Fields<T, MAX, H, N>> {
        use alloc::raw::Recipe as _;
        alloc::raw::Root::<Self>::new(1)
            .tail::<mem::MaybeUninit<T>>(capacity)
            .tail::<IndexedMeta<MAX, H>>(capacity)
            .tail::<[H::Head; N]>(1)
    }
}

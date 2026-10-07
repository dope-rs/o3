use crate::{
    alloc,
    depot::{
        self, capacity,
        queue::{
            indexed::planes::{bound, resident},
            links,
        },
    },
    mem::{self, slot},
};

// SAFETY: the retained proof names an initialized row in this owner's active
// prefix. Consuming the owner prevents lifecycle changes until the scope ends.
unsafe impl<'storage, 'id, T, M, const N: usize, P: Ord, C: alloc::raw::Extent, const E: usize>
    depot::raw::Project<
        links::Index,
        resident::Scope<'storage, 'id, T, M, N, bound::Active<'storage, P, C>, E>,
    > for bound::Scope<'storage, 'id, slot::Storage<T, M>, N, bound::Active<'storage, P, C>>
{
    type Output = (
        resident::Scope<'storage, 'id, T, M, N, bound::Active<'storage, P, C>, E>,
        resident::Key<'id>,
    );

    unsafe fn project(self, index: links::Index) -> Self::Output {
        let key = bound::ActiveKey(capacity::Key::new(unsafe { depot::raw::Proof::new(index) }));
        (self.resident(), resident::Key(key))
    }
}

// SAFETY: the source's shared loan freezes the active prefix and occupancy.
unsafe impl<
    'loan,
    'storage,
    'id,
    T,
    M,
    const N: usize,
    P: Ord,
    C: alloc::raw::Extent,
    const E: usize,
>
    depot::raw::Project<
        links::Index,
        resident::Scope<'storage, 'id, T, M, N, bound::Active<'storage, P, C>, E>,
    >
    for &'loan bound::Scope<'storage, 'id, slot::Storage<T, M>, N, bound::Active<'storage, P, C>>
{
    type Output = &'loan T;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        let key = bound::ActiveKey(capacity::Key::new(unsafe { depot::raw::Proof::new(index) }));
        unsafe { mem::raw::project(self.get(key)) }
    }
}

// SAFETY: this resident owner retains the same initialized row throughout the loan.
unsafe impl<
    'loan,
    'storage,
    'id,
    T,
    M,
    const N: usize,
    P: Ord,
    C: alloc::raw::Extent,
    const E: usize,
>
    depot::raw::Project<
        links::Index,
        resident::Scope<'storage, 'id, T, M, N, bound::Active<'storage, P, C>, E>,
    > for &'loan resident::Scope<'storage, 'id, T, M, N, bound::Active<'storage, P, C>, E>
{
    type Output = &'loan T;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        unsafe {
            depot::raw::Project::<
                links::Index,
                resident::Scope<'storage, 'id, T, M, N, bound::Active<'storage, P, C>, E>,
            >::project(&self.source, index)
        }
    }
}

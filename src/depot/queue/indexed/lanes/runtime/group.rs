use crate::depot::{
    self,
    index::Initialized,
    queue::{indexed::lanes::runtime::Fifo, links, tagged},
};

/// One consecutive group retained under its original exclusive owner loan.
pub struct Group<
    'a,
    T,
    C: depot::raw::Count,
    L: depot::raw::Count,
    H: tagged::raw::Head,
    const N: usize,
    const VACANT: bool = false,
> {
    owner: &'a mut Fifo<T, C, L, H>,
    first: links::Index,
}

impl<'a, T, C: depot::raw::Count, L: depot::raw::Count, H: tagged::raw::Head, const N: usize>
    Group<'a, T, C, L, H, N>
{
    /// # Safety
    /// The N consecutive nodes starting at first belong to this exact owner.
    /// The retained owner cannot be replaced or accessed independently during this loan.
    pub(in crate::depot) unsafe fn new(
        owner: &'a mut Fifo<T, C, L, H>,
        first: links::Index,
    ) -> Self {
        const { assert!(N != 0 && N <= u32::MAX as usize) };
        Self { owner, first }
    }

    /// Removes every group member before lending the wholly vacant publication phase.
    pub fn clear(self) -> Group<'a, T, C, L, H, N, true> {
        let Self { owner, first } = self;
        for offset in 0..N {
            // SAFETY: Group retains this consecutive represented range in the same owner.
            let index = unsafe { links::Index::new_usize(first.get() + offset) };
            if unsafe { depot::raw::LaneStore::nodes(&*owner).get_unchecked(index.get()) }
                .hook
                .is_linked(index)
            {
                // SAFETY: the admitted live hook retains its initialized payload until removal.
                let entry = unsafe {
                    tagged::Linked::new(depot::raw::Proof::<_, Initialized<links::Chain>>::new((
                        &mut *owner,
                        index,
                    )))
                };
                drop(entry.remove());
            }
        }
        Group { owner, first }
    }
}

impl<T, C: depot::raw::Count, L: depot::raw::Count, H: tagged::raw::Head, const N: usize>
    Group<'_, T, C, L, H, N, true>
{
    /// Publishes present values in ordinal order after the complete group was cleared.
    pub fn insert<W>(self, values: [Option<W>; N], mut prepare: impl FnMut(W) -> (u64, T)) {
        let Self { owner, first } = self;
        for (offset, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                let (hash, value) = prepare(value);
                // SAFETY: the consumed group proves this exact node detached and uninitialized.
                let index = unsafe { links::Index::new_usize(first.get() + offset) };
                let entry = unsafe { owner.bucket_mut(hash).at::<false>(index) };
                unsafe { tagged::Vacant::new(entry) }.insert::<true>(value);
            }
        }
    }
}

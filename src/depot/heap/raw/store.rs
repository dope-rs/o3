use std::{mem, ptr};

use crate::{
    alloc,
    depot::{heap, index::Position},
};

/// The disjoint planes of one initialized heap allocation.
/// # Safety
/// The prefix and reverse map form a bijection within the same capacity.
pub(in crate::depot) unsafe trait Store: Sized {
    type Key: crate::depot::raw::Address;
    type Priority: Ord;
    type Position: Position;
    /// The exact allocation shape whose root, tail and extra planes this store owns.
    type Storage: alloc::raw::Shape<Root = heap::Stored<Self::Key, Self::Priority>, Tail = Self::Position>;
    /// The auxiliary plane retained by the backing block's state.
    type Extra;

    fn precedes(_: &Self::Key, _: &Self::Key) -> bool {
        false
    }

    /// The backing block retaining this store's disjoint planes.
    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra>;

    /// The same backing block under this exclusive owner borrow.
    /// Restore the initialized prefix and reverse-map bijection before releasing the loan.
    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra>;

    fn values(&self) -> &[mem::MaybeUninit<heap::Stored<Self::Key, Self::Priority>>] {
        let block = self.storage();
        // SAFETY: the retained prefix length is bounded by this backing allocation.
        unsafe { block.as_ref().get_unchecked(..block.state as usize) }
    }

    /// The initialized reverse map under the same owner loan.
    fn positions(&self) -> &[Self::Position] {
        self.storage().tail()
    }

    /// The minimum entry retaining its stored identity and priority.
    fn peek(&self) -> Option<(Self::Key, &Self::Priority)> {
        // SAFETY: every element of this exact owner's prefix is initialized.
        let entry = unsafe { self.values().first()?.assume_init_ref() };
        Some((entry.identity, &entry.priority))
    }

    /// Resolves the current generation at a physical index, without comparing identities.
    fn current(&self, index: usize) -> Option<&heap::Stored<Self::Key, Self::Priority>> {
        use crate::depot::raw::{Link as _, Word as _};

        let position = self.positions().get(index)?.item()?;
        // SAFETY: the retained reverse map points into this exact initialized prefix.
        Some(unsafe {
            self.values()
                .get_unchecked(position.offset())
                .assume_init_ref()
        })
    }

    /// # Safety
    /// The key and destination (or NONE) belong to this map's exact owner.
    /// Complete that heap mutation before releasing its exclusive planes.
    unsafe fn reindex(map: &mut [Self::Position], key: Self::Key, position: Self::Position) {
        use crate::depot::raw::Address as _;

        unsafe { *map.get_unchecked_mut(key.index()) = position };
    }

    /// Runs required priority destruction before terminal deallocation.
    /// # Safety
    /// This terminal owner deallocates its backing without observing or destroying the entries again.
    unsafe fn destroy(&mut self) {
        if mem::needs_drop::<Self::Priority>() {
            let (len, entries, _) = self.storage_mut().parts_mut();
            // SAFETY: this terminal owner retains the initialized prefix until
            // each priority is destroyed once in the original reverse order.
            let entries = unsafe { entries.get_unchecked_mut(..*len as usize) };
            for entry in entries.iter_mut().rev() {
                unsafe { entry.assume_init_drop() };
            }
        }
    }

    fn clear(&mut self) {
        use crate::depot::raw::Link as _;

        let (len, entries, map) = self.storage_mut().parts_mut();
        while *len != 0 {
            *len -= 1;
            let entry = unsafe { entries.get_unchecked_mut(*len as usize).assume_init_mut() };
            // SAFETY: this exclusive owner retains the initialized entry and its
            // exact reverse map until retirement precedes native destruction.
            unsafe {
                Self::reindex(map, entry.identity, Self::Position::NONE);
                ptr::drop_in_place(entry);
            }
        }
    }
}

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

    fn precedes(left: &Self::Key, right: &Self::Key) -> bool;

    /// The backing block retaining this store's disjoint planes.
    fn storage(&self) -> &alloc::raw::Block<Self::Storage, u32, Self::Extra>;

    /// The same backing block under this exclusive owner borrow.
    fn storage_mut(&mut self) -> &mut alloc::raw::Block<Self::Storage, u32, Self::Extra>;

    /// The prefix length, backing entries and reverse map belong to this exact owner.
    fn view(&self) -> View<'_, Self> {
        let block = self.storage();
        (&block.state, block.as_ref(), block.tail())
    }

    fn values(&self) -> &[mem::MaybeUninit<heap::Stored<Self::Key, Self::Priority>>] {
        let view = self.view();
        // SAFETY: the retained prefix length is bounded by this backing allocation.
        unsafe { view.1.get_unchecked(..*view.0 as usize) }
    }

    /// The initialized reverse map returned by `parts`, under the same owner loan.
    fn positions(&self) -> &[Self::Position] {
        self.view().2
    }

    fn first(&self) -> Option<&heap::Stored<Self::Key, Self::Priority>> {
        // SAFETY: every element of this exact owner's prefix is initialized.
        Some(unsafe { self.values().first()?.assume_init_ref() })
    }

    /// The minimum entry retaining its stored identity and priority.
    fn peek(&self) -> Option<(Self::Key, &Self::Priority)> {
        let entry = self.first()?;
        Some((entry.identity, &entry.priority))
    }

    /// Resolves the current generation at a physical index, without comparing identities.
    fn current(&self, index: usize) -> Option<&heap::Stored<Self::Key, Self::Priority>> {
        use crate::depot::raw::{Link as _, Word as _};

        let position = *self.positions().get(index)?;
        if position == Self::Position::NONE {
            return None;
        }
        // SAFETY: the retained reverse map points into this exact initialized prefix.
        Some(unsafe {
            self.values()
                .get_unchecked(position.offset())
                .assume_init_ref()
        })
    }

    /// Restore the initialized prefix and reverse-map bijection before releasing the loan.
    unsafe fn parts(&mut self) -> Edit<'_, Self> {
        self.storage_mut().parts_mut()
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
        let (len, entries, map) = unsafe { self.parts() };
        while *len != 0 {
            *len -= 1;
            let entry = unsafe { entries.get_unchecked_mut(*len as usize).assume_init_mut() };
            // SAFETY: this exclusive owner retains the initialized entry and its
            // exact reverse map until retirement precedes native destruction.
            unsafe {
                heap::raw::Reindex::<Self>::new(map).removed(entry);
                ptr::drop_in_place(entry);
            }
        }
    }
}

/// Shared and exclusive views retain the same heap prefix and reverse-map bijection.
type View<'a, S> = (
    &'a u32,
    &'a [mem::MaybeUninit<heap::Stored<<S as Store>::Key, <S as Store>::Priority>>],
    &'a [<S as Store>::Position],
);
type Edit<'a, S> = (
    &'a mut u32,
    &'a mut [mem::MaybeUninit<heap::Stored<<S as Store>::Key, <S as Store>::Priority>>],
    &'a mut [<S as Store>::Position],
);

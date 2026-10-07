use std::{mem, num::NonZero};

use crate::depot::queue::{
    group::{
        capacity,
        map::{self, mask},
    },
    indexed::slab,
    links,
};

/// Native branch identities retained throughout a path borrow.
/// # Safety
/// Binding preserves the exact source and its decreasing-mask branch topology.
pub(in crate::depot::queue::group::map) unsafe trait Source {
    type Key: Copy;
    fn bind(&self, node: &map::Node<map::BranchTag>) -> Self::Key;
    /// # Safety
    /// The key was bound in this unchanged source.
    unsafe fn branch(&self, key: Self::Key) -> &map::Branch;
}

// SAFETY: the ledger's affine branch links retain this exact native FIFO.
unsafe impl<const LANES: usize> Source
    for slab::Slab<
        map::Branch,
        map::BranchTag,
        { u32::MAX },
        links::Hook,
        0,
        capacity::Count<LANES>,
    >
{
    type Key = links::Index;
    fn bind(&self, node: &map::Node<map::BranchTag>) -> Self::Key {
        node.bind(self).key().0
    }
    unsafe fn branch(&self, key: Self::Key) -> &map::Branch {
        unsafe { crate::depot::raw::Proof::new((self, key)) }.into()
    }
}

// SAFETY: the insertion-only scope keeps every branded branch key live.
unsafe impl<'id, const LANES: usize> Source
    for slab::Scope<'_, 'id, map::Branch, map::BranchTag, { u32::MAX }, capacity::Count<LANES>>
{
    type Key = slab::Key<'id, map::BranchTag>;
    fn bind(&self, node: &map::Node<map::BranchTag>) -> Self::Key {
        node.bind(self).into()
    }
    unsafe fn branch(&self, key: Self::Key) -> &map::Branch {
        self.get(key)
    }
}

/// Resolves an owner before transferring its final branch and parent into a cursor.
pub(in crate::depot::queue::group::map) fn select<'a, R, const LANES: usize>(
    source: &'a mut map::Branches<LANES>,
    root: &map::Node<map::BranchTag>,
    key: u64,
    select: impl FnOnce(&map::Node<map::Tag>) -> R,
) -> (
    R,
    slab::Cursor<'a, map::Branch, map::BranchTag, { u32::MAX }, capacity::Count<LANES>>,
) {
    let mut index = source.bind(root);
    let mut parent = None;
    let owner = loop {
        // SAFETY: every branch identity was bound in this unchanged, exclusively retained source.
        match unsafe { source.branch(index) }.child(key) {
            map::Link::Owner(owner) => break select(owner),
            map::Link::Branch(branch) => {
                parent = Some(index);
                index = source.bind(branch);
            }
        }
    };
    // SAFETY: decreasing masks retain distinct initialized branch and parent positions;
    // owner selection has completed before transferring the exact exclusive source loan.
    let cursor = unsafe { crate::depot::raw::Proof::new((source, (index, parent))) }.into();
    (owner, cursor)
}

pub(in crate::depot::queue::group::map) struct Path<
    'a,
    'p,
    const LANES: usize,
    S: Source = slab::Slab<
        map::Branch,
        map::BranchTag,
        { u32::MAX },
        links::Hook,
        0,
        capacity::Count<LANES>,
    >,
> {
    source: &'a mut S,
    indices: &'p mut [mem::MaybeUninit<S::Key>; u64::BITS as usize],
    len: NonZero<usize>,
}

impl<'a, 'p, const LANES: usize, S: Source> Path<'a, 'p, LANES, S> {
    pub(in crate::depot::queue::group::map) fn new(
        source: &'a mut S,
        root: &map::Node<map::BranchTag>,
        indices: &'p mut [mem::MaybeUninit<S::Key>; u64::BITS as usize],
    ) -> Self {
        indices[0].write(source.bind(root));
        Self {
            source,
            indices,
            len: NonZero::<usize>::MIN,
        }
    }

    /// Walks to the owner of `key` while retaining every branch on the path.
    pub(in crate::depot::queue::group::map) fn walk(&mut self, key: u64) -> &map::Node<map::Tag> {
        let mut current = self.at(self.len);
        let source = &mut self.source;
        let indices = &mut self.indices;
        let len = &mut self.len;
        loop {
            let branch = match unsafe { source.branch(current) }.child(key) {
                map::Link::Owner(owner) => return owner,
                map::Link::Branch(branch) => branch,
            };
            current = source.bind(branch);
            let position = *len;
            // SAFETY: decreasing one-bit masks bound this initialized path to 64 entries.
            unsafe {
                indices.get_unchecked_mut(position.get()).write(current);
            }
            *len = unsafe { NonZero::new_unchecked(position.get() + 1) };
        }
    }

    /// Retains the insertion path and its native parent, or selects a new root.
    pub(in crate::depot::queue::group::map) fn rewind(
        &mut self,
        mask: mask::Mask,
    ) -> Option<S::Key> {
        let mut position = self.len;
        loop {
            // SAFETY: every initialized path key was bound in this exclusively retained source.
            if unsafe { self.source.branch(self.at(position)) }.mask > mask {
                self.len = position;
                return Some(self.at(position));
            }
            let Some(parent) = NonZero::new(position.get() - 1) else {
                self.len = NonZero::<usize>::MIN;
                return None;
            };
            position = parent;
        }
    }

    fn at(&self, position: NonZero<usize>) -> S::Key {
        // SAFETY: callers retain one position in this path's initialized prefix.
        unsafe {
            self.indices
                .get_unchecked(position.get() - 1)
                .assume_init_read()
        }
    }
}

impl<'a, const LANES: usize> Path<'a, '_, LANES> {
    pub(in crate::depot::queue::group::map) fn into_cursor(
        self,
    ) -> slab::Cursor<'a, map::Branch, map::BranchTag, { u32::MAX }, capacity::Count<LANES>> {
        let index = self.at(self.len);
        let parent = NonZero::new(self.len.get() - 1).map(|parent| self.at(parent));
        // SAFETY: the consumed path transfers its distinct initialized positions
        // and exact exclusive allocation owner into the native cursor.
        unsafe { crate::depot::raw::Proof::new((self.source, (index, parent))) }.into()
    }
}

const _: () = assert!(size_of::<Path<'static, 'static, 1>>() == 3 * size_of::<usize>());

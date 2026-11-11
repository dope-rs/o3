use std::{mem, num::NonZero};

use crate::depot::queue::{
    group::map::{self, mask},
    links,
};

pub(in crate::depot::queue::group::map) fn select<'a, const LANES: usize>(
    source: &'a map::Branches<LANES>,
    root: &map::Node<map::BranchTag>,
    key: u64,
) -> (&'a map::Node<map::Tag>, map::Place) {
    let mut index = root.bind(source).key().0;
    let mut parent = None;
    loop {
        // SAFETY: each branch link is initialized in this retained, unchanged source.
        let branch: &map::Branch = unsafe { crate::depot::raw::Proof::new((source, index)) }.into();
        match branch.child(key) {
            map::Link::Owner(owner) => return (owner, (index, parent)),
            map::Link::Branch(branch) => {
                parent = Some(index);
                index = branch.bind(source).key().0;
            }
        }
    }
}

pub(in crate::depot::queue::group::map) struct Path<'a, 'p, const LANES: usize> {
    pub(super) source: &'a mut map::Branches<LANES>,
    indices: &'p mut [mem::MaybeUninit<links::Index>; u64::BITS as usize],
    len: NonZero<usize>,
}

impl<'a, 'p, const LANES: usize> Path<'a, 'p, LANES> {
    pub(in crate::depot::queue::group::map) fn new(
        source: &'a mut map::Branches<LANES>,
        root: &map::Node<map::BranchTag>,
        indices: &'p mut [mem::MaybeUninit<links::Index>; u64::BITS as usize],
    ) -> Self {
        indices[0].write(root.bind(&*source).key().0);
        Self {
            source,
            indices,
            len: NonZero::<usize>::MIN,
        }
    }

    /// Walks to the owner of `key` while retaining every branch on the path.
    pub(in crate::depot::queue::group::map) fn walk(&mut self, key: u64) -> &map::Node<map::Tag> {
        let mut current = self.at(self.len);
        let source = &*self.source;
        let indices = &mut self.indices;
        let len = &mut self.len;
        loop {
            // SAFETY: every stored identity was bound in this unchanged source.
            let branch: &map::Branch =
                unsafe { crate::depot::raw::Proof::new((source, current)) }.into();
            let branch = match branch.child(key) {
                map::Link::Owner(owner) => return owner,
                map::Link::Branch(branch) => branch,
            };
            current = branch.bind(source).key().0;
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
    ) -> Option<links::Index> {
        let mut position = self.len;
        loop {
            // SAFETY: this position retains a live branch in the exclusive path.
            let branch: &map::Branch =
                unsafe { crate::depot::raw::Proof::new((&*self.source, self.at(position))) }.into();
            if branch.mask > mask {
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

    fn at(&self, position: NonZero<usize>) -> links::Index {
        // SAFETY: callers retain one position in this path's initialized prefix.
        unsafe {
            self.indices
                .get_unchecked(position.get() - 1)
                .assume_init_read()
        }
    }
}

const _: () = assert!(size_of::<Path<'static, 'static, 1>>() == 3 * size_of::<usize>());

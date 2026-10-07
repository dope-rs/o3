use std::mem;

use crate::depot::queue::group::map::{BranchTag, Link, Node, Tag, mask};

pub(in crate::depot::queue::group) struct Branch {
    pub(in crate::depot::queue::group::map) mask: mask::Mask,
    children: (Link, Link),
}

impl Branch {
    pub(in crate::depot::queue::group::map) fn insert(
        parent: Option<&mut Self>,
        root: &mut Link,
        mask: mask::Mask,
        branch: Node<BranchTag>,
        owner: Node<Tag>,
        key: u64,
    ) -> Self {
        let link = Link::Branch(branch);
        let old = match parent {
            Some(parent) => mem::replace(parent.child_mut(key), link),
            None => mem::replace(root, link),
        };
        let added = Link::Owner(owner);
        let children = if mask.contains(key) {
            (old, added)
        } else {
            (added, old)
        };
        Self { mask, children }
    }
    pub(in crate::depot::queue::group::map) fn child(&self, key: u64) -> &Link {
        if self.mask.contains(key) {
            &self.children.1
        } else {
            &self.children.0
        }
    }
    pub(in crate::depot::queue::group::map) fn collapse(&mut self, key: u64, target: &mut Link) {
        let sibling = if self.mask.contains(key) {
            &mut self.children.0
        } else {
            &mut self.children.1
        };
        mem::swap(sibling, target);
    }
    pub(in crate::depot::queue::group::map) fn child_mut(&mut self, key: u64) -> &mut Link {
        if self.mask.contains(key) {
            &mut self.children.1
        } else {
            &mut self.children.0
        }
    }
    pub(in crate::depot::queue::group::map) fn sibling(self, key: u64) -> Link {
        if self.mask.contains(key) {
            self.children.0
        } else {
            self.children.1
        }
    }
}

const _: () = assert!(mem::size_of::<Branch>() == 3 * mem::size_of::<usize>());

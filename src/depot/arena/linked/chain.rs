use std::{cell, mem};

use crate::alloc::tail;

/// Header and links owned by one chain allocation.
#[repr(C)]
pub struct Node<H, T> {
    next: Option<tail::Block<Node<H, T>, T>>,
    header: H,
}

/// Structural ownership of a linked set of tail allocations.
#[repr(transparent)]
pub struct List<H, T> {
    head: cell::UnsafeCell<Option<tail::Block<Node<H, T>, T>>>,
}

/// Physical identity of one allocation branded by its owning list borrow.
pub struct Link<'a, H, T> {
    view: tail::View<'a, Node<H, T>, T>,
}

struct Iter<'a, H, T> {
    next: Option<tail::View<'a, Node<H, T>, T>>,
}

impl<H, T> Node<H, T> {
    pub const fn new(header: H) -> Self {
        Self { next: None, header }
    }

    pub fn header(&self) -> &H {
        &self.header
    }
}

impl<H, T> Default for List<H, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<H, T> List<H, T> {
    pub const fn new() -> Self {
        Self {
            head: cell::UnsafeCell::new(None),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = Link<'_, H, T>> {
        // SAFETY: moving the `Block` token through the cell never moves its
        // allocation, and a shared list borrow excludes terminal list drop.
        let head = unsafe { &*self.head.get() };
        Iter {
            next: head.as_ref().map(tail::View::new),
        }
    }

    pub fn push(&self, mut block: tail::Block<Node<H, T>, T>) -> Link<'_, H, T> {
        // SAFETY: this leaf invokes no callbacks; links borrow allocations,
        // never the head token, and a shared list borrow prevents teardown.
        let head = unsafe { &mut *self.head.get() };
        block.header_mut().next = head.take();
        Link {
            view: tail::View::new(head.insert(block)),
        }
    }
}

impl<H, T> Drop for List<H, T> {
    fn drop(&mut self) {
        let head = self.head.get_mut();
        while let Some(mut owner) = head.take() {
            *head = owner.header_mut().next.take();
        }
    }
}

impl<'a, H, T> Link<'a, H, T> {
    pub fn header(&self) -> &'a H {
        &self.view.header().header
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.view.len()
    }

    pub fn as_slice(&self) -> &'a [T] {
        self.view.as_slice()
    }
}

impl<H, T> Clone for Link<'_, H, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H, T> Copy for Link<'_, H, T> {}

impl<'a, H, T> Iterator for Iter<'a, H, T> {
    type Item = Link<'a, H, T>;

    fn next(&mut self) -> Option<Self::Item> {
        let view = self.next?;
        self.next = view.header().next.as_ref().map(tail::View::new);
        Some(Link { view })
    }
}

const _: () = {
    assert!(mem::size_of::<tail::Block<Node<usize, usize>, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<List<usize, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Link<'_, usize, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Link<'_, usize, usize>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Iter<'_, usize, usize>>() == mem::size_of::<usize>());
};

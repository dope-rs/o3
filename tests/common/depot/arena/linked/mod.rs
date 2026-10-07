use std::{cell::Cell, marker::PhantomData, num::NonZero};

use o3::depot::{
    self,
    arena::{
        self,
        linked::{fifo::ends, head, slots},
    },
};

pub enum Tag {}

pub struct Node<T> {
    pub key: u64,
    pub value: T,
}

pub type Pool<T> = depot::raw::Arena<Node<T>, Tag>;

pub struct Head {
    raw: head::List<Tag>,
}

impl Default for Head {
    fn default() -> Self {
        Self::new()
    }
}

impl Head {
    pub const fn new() -> Self {
        Self {
            raw: head::List::new(),
        }
    }

    /// # Safety
    /// The topology and every retained node must belong to this exact pool.
    pub unsafe fn try_insert<T>(
        &mut self,
        pool: &Pool<T>,
        key: u64,
        value: T,
    ) -> Result<(), Node<T>> {
        let Some(vacant) = pool.vacant() else {
            return Err(Node { key, value });
        };
        let mut cursor = unsafe { pool.cursor(&mut self.raw) };
        while cursor.current().is_some_and(|node| node.key < key) {
            cursor.advance();
        }
        slots::Cursor::insert(unsafe {
            depot::raw::Proof::new((&mut cursor, vacant.insert(Node { key, value })))
        });
        Ok(())
    }

    /// # Safety
    /// The topology and every retained node must belong to this exact pool.
    pub unsafe fn try_push<T>(
        &mut self,
        pool: &Pool<T>,
        key: u64,
        value: T,
    ) -> Result<(), Node<T>> {
        let Some(vacant) = pool.vacant() else {
            return Err(Node { key, value });
        };
        let mut cursor = unsafe { pool.cursor(&mut self.raw) };
        slots::Cursor::insert(unsafe {
            depot::raw::Proof::new((&mut cursor, vacant.insert(Node { key, value })))
        });
        Ok(())
    }

    /// # Safety
    /// The topology and every retained node must belong to this exact pool.
    pub unsafe fn pop<T>(&mut self, pool: &Pool<T>) -> Option<(u64, T)> {
        let node = unsafe { pool.pop_front(&mut self.raw) }?;
        Some((node.key, node.value))
    }

    /// # Safety
    /// The topology and every retained node must belong to this exact pool.
    pub unsafe fn iter<'a, T>(&'a self, pool: &'a Pool<T>) -> impl Iterator<Item = (u64, &'a T)> {
        unsafe { pool.iter(&self.raw) }.map(|node| (node.key, &node.value))
    }
}
pub mod reserve;

pub const NONE: u32 = <u32 as depot::raw::Link>::NONE;

pub struct Slots<'a>(pub &'a [Cell<u32>]);

impl Slots<'_> {
    /// # Safety
    /// `chain` must own only these exact links, and `index` must name a detached slot in them.
    pub unsafe fn push_back(self, chain: &mut ends::Exclusive, index: u32) {
        unsafe {
            ends::Exclusive::push_back(depot::raw::Proof::new((chain, index, |tail| {
                self.0[index as usize].set(NONE);
                if let Some(tail) = tail {
                    self.0[tail as usize].set(index);
                }
            })));
        }
    }

    /// # Safety
    /// `chain` and every retained node must belong to these exact links.
    pub unsafe fn pop_front(self, chain: &mut ends::Exclusive) -> Option<u32> {
        let front = chain.front()?;
        Some(unsafe {
            ends::Front::pop(depot::raw::Proof::new((front, (), |index, ()| {
                (self.0[index as usize].replace(NONE), index)
            })))
        })
    }
}

unsafe impl<T> depot::raw::HeadPop<u32, T> for Slots<'_> {
    type Output = (u32, T);

    unsafe fn pop(self, index: u32, input: T) -> (u32, Self::Output) {
        let next = self.0[index as usize].replace(NONE);
        (next, (index, input))
    }
}

unsafe impl depot::raw::HeadPush<u32, u32> for Slots<'_> {
    unsafe fn push(self, index: u32, head: u32) -> u32 {
        self.0[index as usize].set(head);
        index
    }
}

pub struct Route<const ID: u8>;

type Brand<'d, const ID: u8> = (Cell<&'d Route<ID>>, *mut Route<ID>);

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Branded<'d, const ID: u8> {
    raw: u32,
    brand: PhantomData<Brand<'d, ID>>,
}

impl<'d, const ID: u8> Branded<'d, ID> {
    pub const fn get(self) -> u32 {
        self.raw
    }

    pub const fn try_new(raw: u32) -> Option<Self> {
        if raw >= 8 {
            return None;
        }
        Some(Self {
            raw,
            brand: PhantomData,
        })
    }

    pub const fn sentinel() -> Self {
        Self {
            raw: 8,
            brand: PhantomData,
        }
    }
}

// SAFETY: the reserved sentinel is outside the live branded range.
unsafe impl<const ID: u8> depot::raw::Link for Branded<'_, ID> {
    type Item = Self;

    const NONE: Self = Self::sentinel();

    fn item(self) -> Option<Self> {
        (self.raw < Self::NONE.raw).then_some(self)
    }

    unsafe fn some(item: Self) -> Self {
        item
    }
}

pub struct FixedNode<'a> {
    pub value: u32,
    counts: &'a Cell<(usize, usize)>,
    next: Option<NonZero<u32>>,
}

// SAFETY: only the owner changes the private link; initialization clears it,
// reset preserves it, and all hooks are total without reentry.
unsafe impl<'a> arena::raw::LinkedNode for FixedNode<'a> {
    type Input = (u32, &'a Cell<(usize, usize)>);

    fn new((value, counts): Self::Input) -> Self {
        let (new, reset) = counts.get();
        counts.set((new.saturating_add(1), reset));
        Self {
            value,
            counts,
            next: None,
        }
    }

    fn reset(&mut self, (value, counts): Self::Input) {
        self.value = value;
        self.counts = counts;
        let (new, reset) = self.counts.get();
        self.counts.set((new, reset.saturating_add(1)));
    }

    fn next(&self) -> Option<NonZero<u32>> {
        self.next
    }

    unsafe fn set_next(&mut self, next: Option<NonZero<u32>>) {
        self.next = next;
    }
}

use std::{array, cell, mem, slice};

use crate::{
    alloc,
    depot::{
        arena::slab::{
            entries,
            key::{self, cycle},
        },
        capacity,
    },
};

/// Heap-allocated slot storage; the default policy.
pub struct Heap;

/// Slot storage carried inline by its owner with a const slot count.
pub struct Inline<const N: usize>;

/// Every slot is constructed together with its storage.
pub struct Eager;

/// Each slot is constructed on first use.
pub struct Lazy;

/// Where one slab keeps its slots.
pub trait Storage: entries::Sealed {
    /// The slot plane of this storage policy under one initialization policy.
    type Plane<T, const MAX: u32, M: entries::Mode, I: Init>: Plane<T, MAX, M>;
}

impl Storage for Heap {
    type Plane<T, const MAX: u32, M: entries::Mode, I: Init> = <I as Init>::Heap<T, MAX, M>;
}

impl<const N: usize> Storage for Inline<N> {
    type Plane<T, const MAX: u32, M: entries::Mode, I: Init> = <I as Init>::Inline<N, T, MAX, M>;
}

impl entries::Sealed for Heap {}

impl<const N: usize> entries::Sealed for Inline<N> {}

/// When one slab constructs each slot.
pub trait Init: entries::Sealed {
    const LAZY: bool;

    /// The heap slot plane of this initialization policy.
    type Heap<T, const MAX: u32, M: entries::Mode>: HeapPlane<T, MAX, M>;

    /// The inline slot plane of this initialization policy.
    type Inline<const N: usize, T, const MAX: u32, M: entries::Mode>: InlinePlane<T, MAX, M>;
}

impl Init for Eager {
    const LAZY: bool = false;

    type Heap<T, const MAX: u32, M: entries::Mode> = HeapEager<T, MAX, M>;

    type Inline<const N: usize, T, const MAX: u32, M: entries::Mode> = InlineEager<N, T, MAX, M>;
}

impl Init for Lazy {
    const LAZY: bool = true;

    type Heap<T, const MAX: u32, M: entries::Mode> = HeapLazy<T, MAX, M>;

    type Inline<const N: usize, T, const MAX: u32, M: entries::Mode> = InlineLazy<N, T, MAX, M>;
}

impl entries::Sealed for Eager {}

impl entries::Sealed for Lazy {}

type Slot<T, const MAX: u32, M> = crate::depot::raw::SlabSlot<
    T,
    key::Generation<MAX>,
    <M as entries::Mode>::Busy,
    <<M as entries::Mode>::Cycle as cycle::Policy>::Retired,
>;

type Shape<T, const MAX: u32, M> = crate::depot::raw::SlabCapacity<
    T,
    key::Generation<MAX>,
    <M as entries::Mode>::Busy,
    <<M as entries::Mode>::Cycle as cycle::Policy>::Retired,
>;

type Prepared<T, const MAX: u32, M> = crate::alloc::raw::Block<
    Shape<T, MAX, M>,
    cell::Cell<u32>,
    (),
    mem::MaybeUninit<Slot<T, MAX, M>>,
>;

type Ready<T, const MAX: u32, M> =
    crate::alloc::raw::Block<Shape<T, MAX, M>, cell::Cell<u32>, (), Slot<T, MAX, M>>;

/// One allocated extent whose every slot is constructed with the storage.
pub struct HeapEager<T, const MAX: u32, M: entries::Mode> {
    block: Ready<T, MAX, M>,
}

impl<T, const MAX: u32, M: entries::Mode> HeapPlane<T, MAX, M> for HeapEager<T, MAX, M> {
    fn try_alloc(
        cap: capacity::Count,
        init: impl Fn(u32) -> Slot<T, MAX, M>,
    ) -> Result<Self, alloc::Error> {
        let extent = crate::depot::raw::SlabCapacity::new(cap)?;
        Ok(Self {
            block: crate::alloc::raw::Block::try_init(extent, cell::Cell::new(0), |index| {
                (
                    init(index as u32),
                    cell::Cell::new(mem::MaybeUninit::uninit()),
                )
            })?,
        })
    }
}

impl<T, const MAX: u32, M: entries::Mode> Plane<T, MAX, M> for HeapEager<T, MAX, M> {
    fn retired(&self) -> Option<&cell::Cell<u32>> {
        Some(&self.block.state)
    }

    fn count(&self) -> capacity::Count {
        self.block.extent().count
    }

    fn tail(&self) -> &[cell::Cell<mem::MaybeUninit<u32>>] {
        self.block.tail()
    }

    fn as_ref(&self) -> &[Slot<T, MAX, M>] {
        self.block.as_ref()
    }
}

/// One allocated extent whose slots are constructed on first use.
pub struct HeapLazy<T, const MAX: u32, M: entries::Mode> {
    block: Prepared<T, MAX, M>,
    built: cell::Cell<u32>,
}

impl<T, const MAX: u32, M: entries::Mode> HeapPlane<T, MAX, M> for HeapLazy<T, MAX, M> {
    fn try_alloc(
        cap: capacity::Count,
        _: impl Fn(u32) -> Slot<T, MAX, M>,
    ) -> Result<Self, alloc::Error> {
        let extent = crate::depot::raw::SlabCapacity::new(cap)?;
        Ok(Self {
            block: crate::alloc::raw::Block::try_fill(extent, cell::Cell::new(0), |_| {
                (
                    mem::MaybeUninit::uninit(),
                    cell::Cell::new(mem::MaybeUninit::uninit()),
                )
            })?,
            built: cell::Cell::new(0),
        })
    }
}

impl<T, const MAX: u32, M: entries::Mode> Plane<T, MAX, M> for HeapLazy<T, MAX, M> {
    fn retired(&self) -> Option<&cell::Cell<u32>> {
        Some(&self.block.state)
    }

    fn count(&self) -> capacity::Count {
        self.block.extent().count
    }

    fn tail(&self) -> &[cell::Cell<mem::MaybeUninit<u32>>] {
        self.block.tail()
    }

    fn as_ref(&self) -> &[Slot<T, MAX, M>] {
        // SAFETY: construction wrote every slot below the built prefix of this
        // allocation, which stays live for this shared borrow.
        unsafe { slice::from_raw_parts(self.block.as_ptr().cast(), self.built() as usize) }
    }

    fn built(&self) -> u32 {
        self.built.get()
    }

    fn push(&self, slot: Slot<T, MAX, M>) -> Option<&Slot<T, MAX, M>> {
        let index = self.built();
        if index as usize >= self.capacity() {
            return None;
        }
        // SAFETY: this new slot is in bounds and outside every published prefix.
        // The allocation pointer writes only this slot without borrowing its neighbors.
        let slot = unsafe { (&mut *self.block.as_ptr().add(index as usize)).write(slot) };
        self.built.set(index + 1);
        Some(slot)
    }
}

/// One inline extent whose every slot is constructed with the storage.
pub struct InlineEager<const N: usize, T, const MAX: u32, M: entries::Mode> {
    slots: [Slot<T, MAX, M>; N],
    cells: [cell::Cell<mem::MaybeUninit<u32>>; N],
}

impl<const N: usize, T, const MAX: u32, M: entries::Mode> InlinePlane<T, MAX, M>
    for InlineEager<N, T, MAX, M>
{
    fn inline(init: impl Fn(u32) -> Slot<T, MAX, M>) -> Self {
        const { assert!(N <= u32::MAX as usize, "inline slot count") };
        Self {
            slots: array::from_fn(|index| init(index as u32)),
            cells: [const { cell::Cell::new(mem::MaybeUninit::uninit()) }; N],
        }
    }
}

impl<const N: usize, T, const MAX: u32, M: entries::Mode> Plane<T, MAX, M>
    for InlineEager<N, T, MAX, M>
{
    fn count(&self) -> capacity::Count {
        capacity::Count::new(N as u32)
    }

    fn tail(&self) -> &[cell::Cell<mem::MaybeUninit<u32>>] {
        &self.cells
    }

    fn as_ref(&self) -> &[Slot<T, MAX, M>] {
        &self.slots
    }
}

/// One inline extent whose slots are constructed on first use.
pub struct InlineLazy<const N: usize, T, const MAX: u32, M: entries::Mode> {
    slots: [cell::UnsafeCell<mem::MaybeUninit<Slot<T, MAX, M>>>; N],
    cells: [cell::Cell<mem::MaybeUninit<u32>>; N],
    built: cell::Cell<u32>,
}

impl<const N: usize, T, const MAX: u32, M: entries::Mode> InlinePlane<T, MAX, M>
    for InlineLazy<N, T, MAX, M>
{
    fn inline(_: impl Fn(u32) -> Slot<T, MAX, M>) -> Self {
        const { assert!(N <= u32::MAX as usize, "inline slot count") };
        Self {
            slots: [const { cell::UnsafeCell::new(mem::MaybeUninit::uninit()) }; N],
            cells: [const { cell::Cell::new(mem::MaybeUninit::uninit()) }; N],
            built: cell::Cell::new(0),
        }
    }
}

impl<const N: usize, T, const MAX: u32, M: entries::Mode> Plane<T, MAX, M>
    for InlineLazy<N, T, MAX, M>
{
    fn count(&self) -> capacity::Count {
        capacity::Count::new(N as u32)
    }

    fn tail(&self) -> &[cell::Cell<mem::MaybeUninit<u32>>] {
        &self.cells
    }

    fn as_ref(&self) -> &[Slot<T, MAX, M>] {
        // SAFETY: construction wrote every slot below the built prefix of this
        // inline extent, which stays live for this shared borrow.
        unsafe { slice::from_raw_parts(self.slots.as_ptr().cast(), self.built() as usize) }
    }

    fn built(&self) -> u32 {
        self.built.get()
    }

    fn push(&self, slot: Slot<T, MAX, M>) -> Option<&Slot<T, MAX, M>> {
        let index = self.built();
        if index as usize >= N {
            return None;
        }
        // SAFETY: this index is below the inline extent and uninitialized.
        let slot = unsafe { (*self.slots.get_unchecked(index as usize).get()).write(slot) };
        self.built.set(index + 1);
        Some(slot)
    }
}

/// The exact slots and construction state retained by one sealed storage policy.
pub trait Plane<T, const MAX: u32, M: entries::Mode>: Sized + entries::Sealed {
    fn count(&self) -> capacity::Count;

    /// Heap planes retain their permanent retirement count in allocation metadata.
    fn retired(&self) -> Option<&cell::Cell<u32>> {
        None
    }

    /// The dense position plane; only the owner's live prefix is guaranteed initialized.
    fn tail(&self) -> &[cell::Cell<mem::MaybeUninit<u32>>];

    /// Every constructed slot of this plane, in storage order.
    fn as_ref(&self) -> &[Slot<T, MAX, M>];

    /// The number of slots constructed so far.
    fn built(&self) -> u32 {
        self.count().raw()
    }

    /// Constructs the next lazy slot; returns `None` for a complete or eager plane.
    fn push(&self, _: Slot<T, MAX, M>) -> Option<&Slot<T, MAX, M>> {
        None
    }

    fn capacity(&self) -> usize {
        self.count().get()
    }

    fn get(&self, index: u32) -> Option<&Slot<T, MAX, M>> {
        self.as_ref().get(index as usize)
    }
}

/// The slot operations of one heap plane: an extent allocated before its slots.
pub trait HeapPlane<T, const MAX: u32, M: entries::Mode>: Plane<T, MAX, M> {
    /// Allocates the complete extent, delegating slot construction to `init`.
    fn try_alloc(
        cap: capacity::Count,
        init: impl Fn(u32) -> Slot<T, MAX, M>,
    ) -> Result<Self, alloc::Error>;
}

/// The slot operations of one inline plane: an extent resident in its owner.
pub trait InlinePlane<T, const MAX: u32, M: entries::Mode>: Plane<T, MAX, M> {
    /// Builds the resident extent, delegating slot construction to `init`.
    fn inline(init: impl Fn(u32) -> Slot<T, MAX, M>) -> Self;
}

impl<T, const MAX: u32, M: entries::Mode> entries::Sealed for HeapEager<T, MAX, M> {}
impl<T, const MAX: u32, M: entries::Mode> entries::Sealed for HeapLazy<T, MAX, M> {}
impl<const N: usize, T, const MAX: u32, M: entries::Mode> entries::Sealed
    for InlineEager<N, T, MAX, M>
{
}
impl<const N: usize, T, const MAX: u32, M: entries::Mode> entries::Sealed
    for InlineLazy<N, T, MAX, M>
{
}

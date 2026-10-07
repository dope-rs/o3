//! Fixed lazy storage and its slot reservations.

use std::{mem, num::NonZero};

use crate::{
    alloc::{self, BoxUninitExt as _},
    base::marker::ThreadBound,
    depot::{
        arena::{self, linked::slots::store::Free},
        array::copy,
        capacity,
    },
};

/// Fixed lazy storage for dropless nodes with an embedded free link.
///
/// Physical indices identify the current slot, not an occupant generation.
pub struct Fixed<N: arena::raw::LinkedNode> {
    nodes: Box<[mem::MaybeUninit<N>]>,
    free: Free<Option<NonZero<u32>>>,
    initialized: u16,
    available: u16,
    _thread: ThreadBound,
}

impl<N: arena::raw::LinkedNode> Fixed<N> {
    /// Admits at most u16::MAX resident slots before allocating their backing.
    pub fn try_new(capacity: capacity::Count) -> Result<Self, alloc::Error> {
        const {
            assert!(!mem::needs_drop::<N>());
            assert!(mem::size_of::<Self>() == 2 * mem::size_of::<usize>() + 8);
        }
        if capacity.raw() > u16::MAX as u32 {
            return Err(alloc::Overflow.into());
        }
        Ok(Self {
            nodes: Box::<[mem::MaybeUninit<N>]>::try_uninit(capacity.get())?,
            free: Free::new(None),
            initialized: 0,
            available: capacity.raw() as u16,
            _thread: ThreadBound::NEW,
        })
    }

    /// Reserves and initializes the complete batch before lending its exact owner.
    pub fn reserve<const C: usize>(
        &mut self,
        inputs: &copy::Vec<N::Input, C>,
    ) -> Option<Batch<'_, N, C>> {
        if inputs.len() > self.available as usize {
            return None;
        }
        let len = inputs.len() as u16;
        self.available = self.available.wrapping_sub(len);
        let mut nodes = [const { mem::MaybeUninit::uninit() }; C];
        for (slot, &input) in nodes.iter_mut().zip(inputs) {
            // SAFETY: admission debited this exact owner's complete demand;
            // each preceding claim consumed one distinct free or fresh slot.
            slot.write(unsafe { self.claim(input) });
        }
        Some(Batch {
            owner: self,
            nodes,
            len,
            next: 0,
        })
    }

    pub fn get(&self, index: NonZero<u32>) -> Option<&N> {
        let index = self.live(index)?;
        // SAFETY: admission retained this same owner's initialized live slot.
        Some(unsafe { self.slot(index) })
    }

    pub fn get_mut(&mut self, index: NonZero<u32>) -> Option<&mut N> {
        let index = self.live(index)?;
        // SAFETY: the exclusive owner retains the admitted initialized live slot.
        Some(unsafe { self.slot_mut(index) })
    }

    pub fn entry(&mut self, index: NonZero<u32>) -> Option<Occupied<'_, N>> {
        let index = self.live(index)?;
        Some(Occupied { owner: self, index })
    }

    fn live(&self, index: NonZero<u32>) -> Option<NonZero<u32>> {
        if index.get() > u32::from(self.initialized) {
            return None;
        }
        // SAFETY: every position below initialized was written exactly once.
        let node = unsafe { self.slot(index) };
        node.next().is_none().then_some(index)
    }

    /// # Safety
    /// This exact owner has already debited one available slot.
    unsafe fn claim(&mut self, input: N::Input) -> NonZero<u32> {
        if let Some(index) = self.free.head {
            // SAFETY: the private free chain contains distinct initialized slots.
            let node = unsafe { self.slot_mut(index) };
            let next = node.next();
            node.reset(input);
            // SAFETY: the exclusive owner removes this free node from its chain.
            unsafe { node.set_next(None) };
            self.free.pop(Some(index), next, None);
            return index;
        }
        let offset = self.initialized as usize;
        // SAFETY: the debited credit and empty free chain prove unused backing.
        unsafe { self.nodes.get_unchecked_mut(offset) }.write(N::new(input));
        self.initialized = self.initialized.wrapping_add(1);
        NonZero::<u32>::MIN.saturating_add(offset as u32)
    }

    /// # Safety
    /// The exact owner retains this distinct initialized live slot for return.
    unsafe fn recycle(&mut self, index: NonZero<u32>) {
        let mut free = self.free;
        let next = free.push(Some(index), None);
        // SAFETY: the caller's live-slot ownership excludes a duplicate return.
        unsafe { self.slot_mut(index).set_next(next) };
        self.free = free;
    }

    fn refund(&mut self, count: u16) {
        self.available = self.available.wrapping_add(count);
    }

    /// # Safety
    /// The index is in this exact owner's initialized prefix.
    unsafe fn slot(&self, index: NonZero<u32>) -> &N {
        unsafe {
            self.nodes
                .get_unchecked(index.get() as usize - 1)
                .assume_init_ref()
        }
    }

    /// # Safety
    /// The index is in this exact owner's initialized prefix.
    unsafe fn slot_mut(&mut self, index: NonZero<u32>) -> &mut N {
        unsafe {
            self.nodes
                .get_unchecked_mut(index.get() as usize - 1)
                .assume_init_mut()
        }
    }
}

/// Initialized slots retained by their exact owner until publication or refund.
#[must_use]
pub struct Batch<'a, N: arena::raw::LinkedNode, const C: usize> {
    owner: &'a mut Fixed<N>,
    nodes: [mem::MaybeUninit<NonZero<u32>>; C],
    len: u16,
    next: u16,
}

impl<N: arena::raw::LinkedNode, const C: usize> Batch<'_, N, C> {
    /// Publishes the next reserved physical slot in request order.
    pub fn take(&mut self) -> Option<NonZero<u32>> {
        if self.next == self.len {
            return None;
        }
        // SAFETY: reservation initialized exactly len <= C entries; next < len.
        let index = unsafe {
            self.nodes
                .get_unchecked(self.next as usize)
                .assume_init_read()
        };
        self.next = self.next.wrapping_add(1);
        Some(index)
    }

    pub fn get(&self, index: NonZero<u32>) -> Option<&N> {
        self.owner.get(index)
    }

    pub fn get_mut(&mut self, index: NonZero<u32>) -> Option<&mut N> {
        self.owner.get_mut(index)
    }
}

impl<N: arena::raw::LinkedNode, const C: usize> Drop for Batch<'_, N, C> {
    fn drop(&mut self) {
        let count = self.len.wrapping_sub(self.next);
        while let Some(index) = self.take() {
            // SAFETY: Batch retains the exact owner and excludes all removal;
            // every unpublished slot stays live until its sole FIFO refund.
            unsafe { self.owner.recycle(index) };
        }
        self.owner.refund(count);
    }
}

/// One admitted initialized live slot under its exact owner's exclusive borrow.
#[must_use]
pub struct Occupied<'a, N: arena::raw::LinkedNode> {
    owner: &'a mut Fixed<N>,
    index: NonZero<u32>,
}

impl<N: arena::raw::LinkedNode> Occupied<'_, N> {
    /// Returns this slot once without repeating its admission.
    pub fn remove(self) {
        // SAFETY: consuming the exact entry transfers its sole return right.
        unsafe { self.owner.recycle(self.index) };
        self.owner.refund(1);
    }
}

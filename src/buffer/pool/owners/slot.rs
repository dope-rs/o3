use std::{cell, marker, num, ptr};

use crate::{
    buffer::{
        pool,
        pool::{owners::Key, state},
        storage::regions,
    },
    mem::rc,
};

#[repr(transparent)]
struct Link(cell::Cell<u32>);

impl Link {
    const fn new(next: Option<Key>) -> Self {
        Self(cell::Cell::new(Key::encode(next)))
    }

    fn clear(&self) {
        self.0.set(Key::<false>::encode(None));
    }

    fn set(&self, next: Option<Key>) {
        self.0.set(Key::encode(next));
    }

    fn insert(&self, key: Key<true>) {
        self.0.set(key.get());
    }
}

#[repr(C, align(8))]
pub(in crate::buffer) struct Slot {
    prefix: regions::Prefix,
    link: Link,
}

// SAFETY: Slot retains its exact initialized prefix and embedded reference domain.
unsafe impl crate::mem::raw::Edge for Slot {
    fn count(&self) -> &crate::mem::raw::Count {
        crate::mem::raw::Edge::count(&self.prefix)
    }
}

impl Slot {
    pub(in crate::buffer) fn capacity(&self) -> num::NonZero<u32> {
        // SAFETY: every Slot is the prefix address of one active slot owner in
        // its published pool, whose immutable header records this capacity.
        unsafe { super::capacity_of(ptr::NonNull::from(self).cast()) }
    }

    const fn new(index: Key, next: Option<Key>) -> Self {
        Self {
            prefix: regions::Prefix::empty(index.get()),
            link: Link::new(next),
        }
    }

    pub(super) unsafe fn unlink(&self) {
        self.link.clear();
    }
}

pub(super) struct Vacant<'a>(&'a Slot);

impl Vacant<'_> {
    pub(super) fn insert(self, key: Key<true>) {
        self.0.link.insert(key);
    }
}

/// Validated current free-list head before its infallible removal.
pub(in crate::buffer::pool) struct Claim<K, S> {
    slots: super::Ptr<K, S>,
    key: Key,
}

/// One freshly initialized nonempty slot list before its first acquisition.
#[repr(transparent)]
pub(in crate::buffer::pool) struct Seed<K, S> {
    slots: super::Ptr<K, S>,
}

/// One newly activated slot edge still carrying its exact pool identity.
#[repr(C)]
pub(in crate::buffer::pool) struct Active<K: crate::buffer::raw::Backing, S> {
    pub(in crate::buffer::pool) slots: super::Ptr<K, S>,
    pub(in crate::buffer::pool) key: Key,
}

/// Exact terminal slot state produced only by the last live edge.
#[repr(C)]
pub(super) struct Retired<K, S> {
    slots: super::Ptr<K, S>,
    key: Key,
}

impl<K: crate::buffer::raw::Backing, S> Claim<K, S> {
    pub(in crate::buffer::pool) fn take(self) -> Active<K, S> {
        use crate::mem::raw::Edge;

        let slot = self.slots.slot(self.key);
        let header = self.slots.header();
        // SAFETY: Claim exclusively owns this exact head and its vacant link.
        unsafe { header.free.pop(slot.link.0.get()) };
        slot.link.clear();
        // SAFETY: Claim exclusively owns this free-list head until activation.
        unsafe { slot.activate() };
        header.available.set(header.available.get() - 1);
        Active {
            slots: self.slots,
            key: self.key,
        }
    }
}

impl<K: crate::buffer::raw::Backing, S> Seed<K, S> {
    /// # Safety
    /// The core must have just published a nonempty Slot list initialized in
    /// index order and must not have exposed any acquisition capability.
    pub(super) const unsafe fn from_ptr(core: super::Ptr<K, S>) -> Self {
        Self { slots: core }
    }

    pub(in crate::buffer::pool) fn take(self) -> (super::Ptr<K, S>, Active<K, S>) {
        let core = self.slots;
        // SAFETY: Seed proves index zero belongs to this exact freshly
        // initialized nonempty list and is its current free-list head.
        let key = unsafe { Key::new(0) };
        let active = Claim {
            slots: self.slots,
            key,
        }
        .take();
        (core, active)
    }
}

impl<K: crate::buffer::raw::Backing, S> Active<K, S> {
    /// # Safety
    /// The caller must transfer one live edge for this exact pool slot.
    pub(super) unsafe fn release(slots: super::Ptr<K, S>, key: Key) {
        use crate::mem::raw::Edge;

        // SAFETY: the caller transfers its one live edge to this exact slot.
        if unsafe { slots.slot(key).release() } != rc::Release::Last {
            return;
        }
        Retired { slots, key }.finish();
    }
}

impl<K: crate::buffer::raw::Backing, S> Drop for Active<K, S> {
    fn drop(&mut self) {
        // SAFETY: Active transfers its one live edge to this exact slot.
        unsafe { Self::release(self.slots, self.key) };
    }
}

impl<K: crate::buffer::raw::Backing, S> Retired<K, S> {
    pub(super) fn finish(self) {
        {
            let header = self.slots.header();
            let slot = self.slots.slot(self.key);
            // SAFETY: Retired owns this exact detached slot until publication.
            unsafe { header.free.push(self.key, |next| slot.link.set(next)) };
            header.available.set(header.available.get() + 1);
        }
        super::Header::retire(self.slots);
    }
}

impl<K> Retired<K, super::Any> {
    /// # Safety
    /// The caller must transfer the terminal edge for this exact slot owner.
    pub(super) unsafe fn from_owner(owner: ptr::NonNull<()>) -> Self {
        // SAFETY: the terminal owner is an active Slot prefix from its live pool.
        let super::Binding { slots, key } =
            unsafe { super::Ptr::<K, super::Any>::from_owner(owner) };
        Self { slots, key }
    }
}

/// One ephemeral projection of an exact live pool slot.
pub(in crate::buffer) struct Entry<'span, K: crate::buffer::raw::Backing, S = state::Uninit> {
    slots: super::Ptr<K, S>,
    pub(in crate::buffer::pool) key: Key,
    live: marker::PhantomData<&'span ()>,
}

impl<'span, K: crate::buffer::raw::Backing, S> Entry<'span, K, S> {
    pub(in crate::buffer::pool) const fn new<M>(span: &'span pool::Lease<K, M, S>) -> Self {
        Self {
            slots: span.slots,
            key: span.key,
            live: marker::PhantomData,
        }
    }

    pub(in crate::buffer::pool) fn capacity(self) -> num::NonZero<u32> {
        self.slots.header().capacity()
    }

    pub(in crate::buffer) fn data(self) -> *mut u8 {
        self.slots.data(self.key)
    }

    pub(in crate::buffer::pool) fn owner(self) -> ptr::NonNull<regions::Prefix> {
        self.slots.owner(self.key)
    }

    pub(in crate::buffer::pool) fn is_unique(self) -> bool {
        self.slots.is_unique(self.key)
    }

    pub(in crate::buffer::pool) fn try_retain(self) -> Result<Active<K, S>, rc::Error> {
        use crate::mem::raw::Edge;

        // SAFETY: the borrowed Span owns this exact active slot throughout admission.
        unsafe { self.slot().prefix.try_retain() }?;
        Ok(Active {
            slots: self.slots,
            key: self.key,
        })
    }

    /// # Safety
    /// The caller must transfer the Span edge projected by this Entry.
    pub(in crate::buffer::pool) unsafe fn release(self) {
        // SAFETY: Entry keeps the exact pool and key paired by its live Span.
        unsafe { Active::release(self.slots, self.key) };
    }

    pub(super) const fn core(self) -> super::Ptr<K, S> {
        self.slots
    }

    pub(super) fn slot(self) -> &'span Slot {
        // SAFETY: Entry is constructed only from a live Span borrow; that span
        // retains this exact slot and its pool for the complete `'span` domain.
        unsafe { self.slots.ptr(self.key).as_ref() }
    }

    pub(super) fn association(self) -> Result<Key<true>, Vacant<'span>> {
        let slot = self.slot();
        // SAFETY: Entry proves this is an active slot, whose link is either
        // empty or an account key published from this exact pool's table.
        unsafe { Key::<true>::decode(slot.link.0.get()) }.ok_or(Vacant(slot))
    }
}

impl<K: crate::buffer::raw::Backing, S> Clone for Entry<'_, K, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: crate::buffer::raw::Backing, S> Copy for Entry<'_, K, S> {}

const _: () = {
    assert!(size_of::<Link>() == size_of::<u32>());
    assert!(size_of::<Slot>() == 16);
    assert!(align_of::<Slot>() == 8);
    assert!(size_of::<Seed<super::Plain, state::Uninit>>() == size_of::<usize>());
    assert!(size_of::<Active<super::Plain, state::Uninit>>() == 2 * size_of::<usize>());
    assert!(size_of::<Retired<super::Plain, state::Uninit>>() == 2 * size_of::<usize>());
    assert!(size_of::<super::Binding<super::Plain>>() == 2 * size_of::<usize>());
    assert!(size_of::<super::Binding<super::Resident<'static>>>() == 2 * size_of::<usize>());
};

impl Slot {
    pub(super) unsafe fn initialize<K>(base: super::Base<K>, slots: u32) {
        // SAFETY: Layout places exactly `slots` Slot cells immediately after
        // the Header cell owned by this unpublished allocation base.
        let base = unsafe { base.slots().as_ptr() };
        for index in 0..slots {
            let next = if index + 1 == slots {
                None
            } else {
                // SAFETY: index + 1 remains within this exact Slot array.
                Some(unsafe { Key::new(index + 1) })
            };
            // SAFETY: the loop index is within this exact Slot array.
            let key = unsafe { Key::new(index) };
            // SAFETY: every index is in the allocated Slot array and is
            // initialized exactly once before the pool is published.
            unsafe { base.add(index as usize).write(Slot::new(key, next)) };
        }
    }

    pub(super) fn key(&self) -> Key {
        // SAFETY: Slot initialization stores its exact immutable key in this
        // typed prefix before the containing pool is published.
        unsafe { Key::new(self.prefix.word()) }
    }
}

impl<K, S> Claim<K, S> {
    pub(in crate::buffer::pool) fn new(slots: super::Ptr<K, S>) -> Option<Self> {
        let key = slots.header().free.head()?;
        Some(Self { slots, key })
    }
}

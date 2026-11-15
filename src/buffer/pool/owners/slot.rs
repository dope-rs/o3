use std::{marker, mem, num, ptr};

use crate::{
    buffer::{
        pool,
        pool::{owners::Key, state},
        storage::regions,
    },
    mem::rc,
};

#[repr(C, align(8))]
pub(in crate::buffer) struct Slot<K: crate::buffer::raw::Backing = super::Plain> {
    prefix: regions::Prefix,
    pub(super) link: K::Link,
}

// SAFETY: Slot retains its exact initialized prefix and embedded reference domain.
unsafe impl<K: crate::buffer::raw::Backing> crate::mem::raw::Edge for Slot<K> {
    fn count(&self) -> &crate::mem::raw::Count {
        crate::mem::raw::Edge::count(&self.prefix)
    }
}

// SAFETY: each slot edge retains its exact published pool until terminal release.
unsafe impl<K: crate::buffer::raw::Backing> crate::mem::raw::Target for Slot<K> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: Last transfers this slot before its live key is replaced.
        unsafe { Retired::<K, super::Any>::from_owner(pointer.cast()) }.finish();
    }
}

/// One freshly initialized nonempty slot list before its first acquisition.
#[repr(transparent)]
pub(in crate::buffer::pool) struct Seed<K, S> {
    slots: super::Ptr<K, S>,
}

/// Exact terminal slot state produced only by the last live edge.
#[repr(C)]
pub(super) struct Retired<K, S> {
    slots: super::Ptr<K, S>,
    key: Key,
}

impl<K: crate::buffer::raw::Backing, S> pool::Span<K, pool::Owned, S> {
    pub(in crate::buffer::pool) fn acquire(slots: super::Ptr<K, S>) -> Option<Self> {
        let key = slots.header().free.head()?;
        // SAFETY: this exact vacant head is consumed without intervening reentry.
        Some(unsafe { Self::take(slots, key) })
    }

    /// # Safety
    /// `key` is this pool's exclusively claimed, initialized free-list head.
    unsafe fn take(slots: super::Ptr<K, S>, key: Key) -> Self {
        use crate::mem::raw::Edge;

        let slot = slots.slot(key);
        let header = slots.header();
        // SAFETY: the caller owns this exact vacant head. Any resident association
        // is empty; only counted spans can issue an alias before publication.
        unsafe {
            header.free.pop(slot.prefix.word());
            slot.prefix.write(key.get());
            if K::COUNTED {
                slot.activate();
            }
        }
        header.available.set(header.available.get() - 1);
        Self {
            slots,
            key,
            len: 0,
            root: pool::Owned,
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

    pub(in crate::buffer::pool) fn take(self) -> pool::Span<K, pool::Owned, S> {
        // SAFETY: Seed proves index zero belongs to this exact freshly
        // initialized nonempty list and is its current free-list head.
        let key = unsafe { Key::new(0) };
        // SAFETY: Seed owns this initialized head until the first acquisition.
        unsafe { pool::Span::take(self.slots, key) }
    }
}

impl<K: crate::buffer::raw::Backing> Slot<K> {
    /// # Safety
    /// The caller must transfer one live edge for this exact pool slot.
    pub(in crate::buffer::pool) unsafe fn release<S>(slots: super::Ptr<K, S>, key: Key) {
        use crate::mem::raw::Edge;

        // SAFETY: the caller transfers its one live edge to this exact slot.
        if K::COUNTED && unsafe { Edge::release(slots.slot(key)) } != rc::Release::Last {
            return;
        }
        Retired { slots, key }.finish();
    }
}

impl<K: crate::buffer::raw::Backing, M, S> pool::Span<K, M, S> {
    pub(in crate::buffer) fn into_edge(self) -> super::Owner<K, M> {
        use crate::mem::raw::Edge;

        let this = mem::ManuallyDrop::new(self);
        if !K::COUNTED {
            // SAFETY: consuming this unique span publishes its first counted edge.
            unsafe { this.entry().slot().activate() };
        }
        super::Owner::new(this.entry().owner())
    }
}

impl<K: crate::buffer::raw::Backing, S> Retired<K, S> {
    pub(super) fn finish(self) {
        {
            let header = self.slots.header();
            let slot = self.slots.slot(self.key);
            // SAFETY: Retired owns this exact detached slot until publication.
            unsafe {
                header
                    .free
                    .push(self.key, |next| slot.prefix.write(Key::encode(next)))
            };
            header.available.set(header.available.get() + 1);
        }
        super::Header::retire(self.slots);
    }
}

impl<K: crate::buffer::raw::Backing> Retired<K, super::Any> {
    /// # Safety
    /// The caller must transfer the terminal edge for this exact slot owner.
    pub(super) unsafe fn from_owner(owner: ptr::NonNull<()>) -> Self {
        // SAFETY: the terminal owner is an active Slot prefix from its live pool.
        let (slots, key) = unsafe { super::Ptr::<K, super::Any>::from_owner(owner) };
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
    pub(in crate::buffer::pool) const fn new<M>(span: &'span pool::Span<K, M, S>) -> Self {
        Self {
            slots: span.slots,
            key: span.key,
            live: marker::PhantomData,
        }
    }

    pub(in crate::buffer::pool) fn capacity(self) -> num::NonZero<u32> {
        self.slots.capacity()
    }

    pub(in crate::buffer) fn data(self) -> *mut u8 {
        self.slots.data(self.key)
    }

    pub(in crate::buffer::pool) fn owner(self) -> ptr::NonNull<regions::Prefix> {
        self.slots.owner(self.key)
    }

    pub(super) const fn core(self) -> super::Ptr<K, S> {
        self.slots
    }

    pub(super) fn slot(self) -> &'span Slot<K> {
        // SAFETY: Entry is constructed only from a live Span borrow; that span
        // retains this exact slot and its pool for the complete `'span` domain.
        unsafe { self.slots.ptr(self.key).as_ref() }
    }
}

impl<K: crate::buffer::raw::Backing, S> Clone for Entry<'_, K, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K: crate::buffer::raw::Backing, S> Copy for Entry<'_, K, S> {}

impl<S> Entry<'_, super::Resident<'_>, S> {
    pub(in crate::buffer::pool) fn is_unique(self) -> bool {
        use crate::mem::raw::Edge;

        self.slot().is_unique()
    }
}

const _: () = {
    assert!(size_of::<Slot>() == 8);
    assert!(size_of::<Slot<super::Resident<'static>>>() == 16);
    assert!(size_of::<Slot<super::Charged<'static>>>() == 8);
    assert!(align_of::<Slot>() == 8);
    assert!(size_of::<Seed<super::Plain, state::Uninit>>() == size_of::<usize>());
    assert!(size_of::<Retired<super::Plain, state::Uninit>>() == 2 * size_of::<usize>());
    assert!(size_of::<super::Binding<super::Plain>>() == 2 * size_of::<usize>());
    assert!(size_of::<super::Binding<super::Resident<'static>>>() == 2 * size_of::<usize>());
};

impl<K: crate::buffer::raw::Backing> Slot<K> {
    pub(super) unsafe fn initialize(base: super::Base<K>, slots: u32, link: K::Link) {
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
            // SAFETY: every index is in the allocated Slot array and is
            // initialized exactly once before the pool is published.
            unsafe {
                base.add(index as usize).write(Self {
                    prefix: regions::Prefix::empty(Key::<false>::encode(next)),
                    link: link.clone(),
                });
            }
        }
    }

    /// # Safety
    /// The slot still contains its live key, before free-list publication.
    pub(super) unsafe fn key(&self) -> Key {
        // SAFETY: the caller retains the exact key published at activation.
        unsafe { Key::new(self.prefix.word()) }
    }
}

use std::{cell, marker, mem, pin, ptr};

use crate::collections::{self, fixed::pinned::recycle, slab};

const OCCUPIED: usize = 1;
type Invariant<'owner> = marker::PhantomData<fn(&'owner ()) -> &'owner ()>;

struct Entry<T> {
    value: T,
    state: cell::Cell<*mut ()>,
}

struct Group<T> {
    entries: pin::Pin<Box<[Entry<T>]>>,
    free: cell::Cell<*mut Entry<T>>,
    _thread: crate::ThreadBound,
}

/// Fixed storage for values which remain pinned and are reset in place.
#[repr(transparent)]
pub struct Pool<T: recycle::Recycle> {
    group: Box<Group<T>>,
}

/// Exclusive ownership of one recyclable slot before it is committed.
#[must_use = "a pinned reservation must be committed or released"]
#[repr(transparent)]
pub struct Reservation<'owner, T: recycle::Recycle> {
    entry: ptr::NonNull<Entry<T>>,
    owner: Invariant<'owner>,
    _thread: crate::ThreadBound,
}

/// Detached exclusive ownership of one pinned recyclable value.
#[must_use = "dropping the lease recycles its pinned value"]
#[repr(transparent)]
pub struct Lease<'owner, T: recycle::Recycle> {
    entry: ptr::NonNull<Entry<T>>,
    owner: Invariant<'owner>,
    _thread: crate::ThreadBound,
}

impl<T: recycle::Recycle> Pool<T> {
    pub fn try_with_capacity(
        capacity: slab::Capacity,
        mut initialize: impl FnMut(usize) -> T,
    ) -> Result<Self, collections::AllocationError> {
        let entries = collections::BoxSliceExt::try_box_with(capacity.get(), |index| Entry {
            value: initialize(index),
            state: cell::Cell::new(ptr::null_mut()),
        })?;
        let mut group: Box<Group<T>> = collections::BoxExt::try_box(Group {
            entries: Box::into_pin(entries),
            free: cell::Cell::new(ptr::null_mut()),
            _thread: crate::ThreadBound::NEW,
        })?;
        group.chain();
        Ok(Self { group })
    }

    pub fn with_capacity(capacity: slab::Capacity, initialize: impl FnMut(usize) -> T) -> Self {
        match Self::try_with_capacity(capacity, initialize) {
            Ok(pool) => pool,
            Err(error) => error.abort(),
        }
    }

    /// Reserves a value while retaining a borrow of this pool.
    pub fn reserve(&self) -> Option<Reservation<'_, T>> {
        Some(Reservation {
            entry: self.group.reserve()?,
            owner: marker::PhantomData,
            _thread: crate::ThreadBound::NEW,
        })
    }

    /// Reserves under an external proof that keeps the pool alive.
    pub fn reserve_owned<'owner>(
        owner: impl recycle::raw::PoolOwner<'owner, T>,
    ) -> Option<Reservation<'owner, T>> {
        let pool = owner.pool();
        // SAFETY: PoolOwner keeps this pool valid for the call and its backing
        // group live for every handle carrying the returned owner brand.
        let entry = unsafe { pool.as_ref() }.group.reserve()?;
        Some(Reservation {
            entry,
            owner: marker::PhantomData,
            _thread: crate::ThreadBound::NEW,
        })
    }

    pub fn capacity(&self) -> usize {
        self.group.entries.len()
    }
}

impl<T> Group<T> {
    fn chain(&mut self) {
        let entries = self.entries.as_mut();
        // SAFETY: the boxed slice is pinned before its intrusive links are
        // built and no API exposes mutable unpinned access afterwards.
        let entries = unsafe { entries.get_unchecked_mut() };
        let mut next: *mut Entry<T> = ptr::null_mut();
        for entry in entries.iter_mut().rev() {
            entry.state.set(next.cast());
            next = entry;
        }
        self.free.set(next);
    }

    fn reserve(&self) -> Option<ptr::NonNull<Entry<T>>> {
        let owner = ptr::NonNull::from(self);
        let entry = ptr::NonNull::new(self.free.get())?;
        // SAFETY: free contains only entries in this pinned group. A free entry
        // stores the next free entry or null and has no live owner.
        let next: *mut Entry<T> = unsafe { entry.as_ref().state.get().cast() };
        debug_assert_eq!(next.addr() & OCCUPIED, 0);
        self.free.set(next);
        // SAFETY: removing the entry from the free list transfers its unique
        // ownership to the returned reservation.
        unsafe {
            entry
                .as_ref()
                .state
                .set(owner.as_ptr().cast::<()>().map_addr(|addr| addr | OCCUPIED));
        }
        Some(entry)
    }
}

impl<'owner, T: recycle::Recycle> Reservation<'owner, T> {
    pub fn get(&self) -> pin::Pin<&T> {
        self.entry.get()
    }

    pub fn get_mut(&mut self) -> pin::Pin<&mut T> {
        self.entry.get_mut()
    }

    pub fn commit(self) -> Lease<'owner, T> {
        let this = mem::ManuallyDrop::new(self);
        Lease {
            entry: this.entry,
            owner: marker::PhantomData,
            _thread: crate::ThreadBound::NEW,
        }
    }
}

impl<T: recycle::Recycle> Drop for Reservation<'_, T> {
    fn drop(&mut self) {
        self.entry.release();
    }
}

impl<T: recycle::Recycle> Lease<'_, T> {
    pub fn get(&self) -> pin::Pin<&T> {
        self.entry.get()
    }

    pub fn get_mut(&mut self) -> pin::Pin<&mut T> {
        self.entry.get_mut()
    }
}

impl<T: recycle::Recycle> Drop for Lease<'_, T> {
    fn drop(&mut self) {
        self.entry.release();
    }
}

trait EntryPointer<T: recycle::Recycle> {
    fn get(&self) -> pin::Pin<&T>;
    fn get_mut(&mut self) -> pin::Pin<&mut T>;
    fn release(self);
}

impl<T: recycle::Recycle> EntryPointer<T> for ptr::NonNull<Entry<T>> {
    fn get(&self) -> pin::Pin<&T> {
        // SAFETY: every live reservation or lease exclusively owns an entry in
        // a pinned boxed slice, and a shared borrow cannot move its value.
        unsafe { pin::Pin::new_unchecked(&self.as_ref().value) }
    }

    fn get_mut(&mut self) -> pin::Pin<&mut T> {
        // SAFETY: the unique reservation or lease owns this occupied entry and
        // the pinned backing slice prevents its value from moving.
        unsafe { pin::Pin::new_unchecked(&mut self.as_mut().value) }
    }

    fn release(mut self) {
        // SAFETY: only the unique live reservation or lease releases an entry.
        // Its tagged state identifies the pinned group kept alive by either the
        // pool borrow or PoolOwner contract.
        unsafe {
            let state = self.as_ref().state.get();
            debug_assert_eq!(state.addr() & OCCUPIED, OCCUPIED);
            let group = ptr::NonNull::<Group<T>>::new_unchecked(
                state.map_addr(|addr| addr & !OCCUPIED).cast::<Group<T>>(),
            );
            T::recycle(pin::Pin::new_unchecked(&mut self.as_mut().value));
            self.as_ref().state.set(group.as_ref().free.get().cast());
            group.as_ref().free.set(self.as_ptr());
        }
    }
}

struct StaticAssert;

impl recycle::Recycle for StaticAssert {
    fn recycle(self: pin::Pin<&mut Self>) {}
}

const _: () = {
    assert!(mem::size_of::<Pool<StaticAssert>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Reservation<'static, StaticAssert>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<'static, StaticAssert>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Lease<'static, StaticAssert>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Entry<StaticAssert>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Group<StaticAssert>>() == 3 * mem::size_of::<usize>());
    assert!(mem::align_of::<Entry<StaticAssert>>() >= 2);
    assert!(mem::align_of::<Group<StaticAssert>>() >= 2);
};

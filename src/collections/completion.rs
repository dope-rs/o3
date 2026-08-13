use std::{cell, marker, mem, process, ptr};

use super::{self as collections, AllocationError};

const OCCUPIED: usize = 1;
const DETACHED: usize = 2;
const TERMINAL: usize = 4;
const FLAGS: usize = OCCUPIED | DETACHED | TERMINAL;

type Invariant<'owner> = marker::PhantomData<fn(&'owner ()) -> &'owner ()>;

struct Entry<T: Copy> {
    value: cell::Cell<mem::MaybeUninit<T>>,
    state: cell::Cell<*mut ()>,
}

struct Group<T: Copy> {
    entries: Box<[Entry<T>]>,
    free: cell::Cell<*mut Entry<T>>,
    leased: cell::Cell<bool>,
    next: cell::Cell<Option<ptr::NonNull<Group<T>>>>,
}

/// Reusable storage for operations that may complete after their Rust owner is
/// dropped.
///
/// Entries remain occupied until both the owner lease is gone and the external
/// producer has reported terminal completion. `Arena` itself does not add a
/// reference count: an [`ArenaOwner`] supplies the surrounding lifetime proof.
pub struct Arena<T: Copy> {
    groups: Option<ptr::NonNull<Group<T>>>,
    owner: marker::PhantomData<Box<Group<T>>>,
}

/// Proof that an arena is owned for `'owner`.
///
/// # Safety
///
/// The returned pointer must be valid and exclusively accessible for the
/// duration of the call that consumes the source. Its arena must remain alive
/// until every handle issued with `'owner` has been dropped and every exposed
/// [`Echo`] has reached terminal completion. Calls using the same arena must
/// not race on different threads.
pub unsafe trait ArenaOwner<'owner, T: Copy> {
    fn arena(self) -> ptr::NonNull<Arena<T>>;
}

/// A leased group of reusable completion entries.
#[repr(transparent)]
pub struct Slots<'owner, T: Copy> {
    group: ptr::NonNull<Group<T>>,
    owner: Invariant<'owner>,
}

/// A completion entry that rolls back unless committed.
#[must_use = "a reserved completion must be committed or released"]
#[repr(transparent)]
pub struct Reservation<'owner, T: Copy> {
    key: Echo<T>,
    owner: Invariant<'owner>,
}

/// The Rust owner's claim on an externally completing entry.
///
/// The lifetime is invariant and cannot be widened independently of the arena
/// owner.
///
/// ```compile_fail
/// use o3::collections::completion::Lease;
///
/// fn widen<'short, 'long, T: Copy>(lease: Lease<'short, T>) -> Lease<'long, T> {
///     lease
/// }
/// ```
#[must_use = "a live completion must reach terminal completion or be detached"]
#[repr(transparent)]
pub struct Lease<'owner, T: Copy> {
    key: Echo<T>,
    owner: Invariant<'owner>,
}

/// A pointer-sized completion key tied to its arena owner.
#[repr(transparent)]
pub struct Key<'owner, T: Copy> {
    echo: Echo<T>,
    owner: Invariant<'owner>,
}

/// A pointer-sized token echoed by an external completion source.
///
/// `Echo` intentionally erases the owner lifetime so it can cross an FFI or
/// kernel boundary. The [`ArenaOwner`] contract keeps its storage alive until
/// terminal completion. Resolving one copy as terminal invalidates every copy
/// of the same token; none may be used afterward.
#[repr(transparent)]
pub struct Echo<T: Copy>(ptr::NonNull<Entry<T>>);

/// Borrowed authority to resolve echoed completion tokens.
pub struct Drain<'arena, T: Copy> {
    arena: marker::PhantomData<&'arena Arena<T>>,
}

/// An echoed token whose resolution is tied to a live arena borrow.
#[repr(transparent)]
pub struct Completion<'arena, T: Copy> {
    key: Echo<T>,
    arena: marker::PhantomData<&'arena Arena<T>>,
}

const _: () = {
    assert!(mem::align_of::<Group<usize>>() >= 8);
    assert!(mem::size_of::<Entry<usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Echo<usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Key<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Lease<'static, usize>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Slots<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Completion<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Drain<'static, usize>>() == 0);
    assert!(mem::size_of::<Arena<usize>>() == mem::size_of::<usize>());
};

impl<T: Copy> Arena<T> {
    pub const fn new() -> Self {
        Self {
            groups: None,
            owner: marker::PhantomData,
        }
    }

    /// Acquires a reusable group using an external owner lifetime.
    pub fn try_slots<'owner>(
        owner: impl ArenaOwner<'owner, T>,
        capacity: usize,
    ) -> Result<Slots<'owner, T>, AllocationError> {
        let mut arena = owner.arena();
        // SAFETY: ArenaOwner provides exclusive access for this call and keeps
        // the arena alive for every handle and external echo issued here.
        unsafe { arena.as_mut() }.try_slots_inner(capacity)
    }

    /// Borrows authority to resolve external completions.
    pub fn drain(&self) -> Drain<'_, T> {
        Drain {
            arena: marker::PhantomData,
        }
    }

    fn try_slots_inner<'owner>(
        &mut self,
        capacity: usize,
    ) -> Result<Slots<'owner, T>, AllocationError> {
        let mut selected = None;
        let mut retained = None;
        let mut quiescent = None;
        let mut cursor = self.groups.take();
        while let Some(group) = Group::pop(&mut cursor) {
            // SAFETY: every pointer in the arena list owns one live group.
            let group_ref = unsafe { group.as_ref() };
            if !group_ref.is_quiescent() {
                Group::push(&mut retained, group);
                continue;
            }
            let len = group_ref.entries.len();
            let better = selected.is_none_or(|selected: ptr::NonNull<Group<T>>| {
                // SAFETY: selected owns a live group detached from every list.
                len < unsafe { selected.as_ref() }.entries.len()
            });
            if len >= capacity && better {
                if let Some(previous) = selected.replace(group) {
                    Group::push(&mut quiescent, previous);
                }
            } else {
                Group::push(&mut quiescent, group);
            }
        }

        let group = match selected {
            Some(group) => group,
            None => match Group::try_new(capacity) {
                Ok(group) => group,
                Err(error) => {
                    self.groups = retained;
                    while let Some(group) = Group::pop(&mut quiescent) {
                        Group::push(&mut self.groups, group);
                    }
                    return Err(error);
                }
            },
        };
        while let Some(group) = Group::pop(&mut quiescent) {
            // SAFETY: quiescent groups have no live slot or entry pointers.
            unsafe { Group::drop_owned(group) };
        }

        // SAFETY: group owns a live allocation detached from every list.
        unsafe { group.as_ref() }.prepare(capacity);
        Group::push(&mut retained, group);
        self.groups = retained;
        Ok(Slots {
            group,
            owner: marker::PhantomData,
        })
    }
}

impl<T: Copy> Drop for Arena<T> {
    fn drop(&mut self) {
        while let Some(group) = Group::pop(&mut self.groups) {
            // SAFETY: ArenaOwner requires every issued handle and external
            // echo to finish before the arena is dropped.
            unsafe { Group::drop_owned(group) };
        }
    }
}

impl<T: Copy> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Copy> Group<T> {
    fn try_new(capacity: usize) -> Result<ptr::NonNull<Self>, AllocationError> {
        let entries = collections::BoxSliceExt::try_box_with(capacity, |_| Entry {
            value: cell::Cell::new(mem::MaybeUninit::uninit()),
            state: cell::Cell::new(ptr::null_mut()),
        })?;
        let group = collections::BoxExt::try_box(Self {
            entries,
            free: cell::Cell::new(ptr::null_mut()),
            leased: cell::Cell::new(false),
            next: cell::Cell::new(None),
        })?;
        // Box::into_raw establishes the sole owning pointer. The allocation is
        // reconstructed only after it becomes quiescent or its arena drops.
        Ok(unsafe { ptr::NonNull::new_unchecked(Box::into_raw(group)) })
    }

    fn pop(list: &mut Option<ptr::NonNull<Self>>) -> Option<ptr::NonNull<Self>> {
        let group = list.take()?;
        // SAFETY: list membership guarantees a live owned group.
        *list = unsafe { group.as_ref() }.next.replace(None);
        Some(group)
    }

    fn push(list: &mut Option<ptr::NonNull<Self>>, group: ptr::NonNull<Self>) {
        // SAFETY: callers pass a live owned group detached from every list.
        let group_ref = unsafe { group.as_ref() };
        debug_assert!(group_ref.next.get().is_none());
        group_ref.next.set(list.take());
        *list = Some(group);
    }

    unsafe fn drop_owned(group: ptr::NonNull<Self>) {
        debug_assert!(unsafe { group.as_ref() }.next.get().is_none());
        // SAFETY: group came from Box::into_raw, remains uniquely owned, and
        // is reconstructed exactly once after all derived pointers expire.
        drop(unsafe { Box::from_raw(group.as_ptr()) });
    }

    fn is_quiescent(&self) -> bool {
        !self.leased.get()
            && self
                .entries
                .iter()
                .all(|entry| entry.state.get().addr() & OCCUPIED == 0)
    }

    fn prepare(&self, capacity: usize) {
        debug_assert!(!self.leased.get());
        debug_assert!(capacity <= self.entries.len());
        let mut next: *mut Entry<T> = ptr::null_mut();
        for entry in self.entries[..capacity].iter().rev() {
            debug_assert_eq!(entry.state.get().addr() & OCCUPIED, 0);
            entry.state.set(next.cast());
            next = ptr::from_ref(entry).cast_mut();
        }
        self.free.set(next);
        self.leased.set(true);
    }
}

impl<'owner, T: Copy> Slots<'owner, T> {
    pub fn reserve(&self, value: T) -> Option<Reservation<'owner, T>> {
        let owner = self.group;
        // SAFETY: ArenaOwner keeps the group allocated while Slots is live.
        let group = unsafe { owner.as_ref() };
        debug_assert!(group.leased.get());
        let key = Echo(ptr::NonNull::new(group.free.get())?);
        // SAFETY: free only contains entries owned by this group.
        let entry = unsafe { key.0.as_ref() };
        let next = entry.state.get().cast::<Entry<T>>();
        debug_assert_eq!(next.addr() & OCCUPIED, 0);
        group.free.set(next);
        entry.value.set(mem::MaybeUninit::new(value));
        entry.state.set(
            owner
                .as_ptr()
                .cast::<()>()
                .map_addr(|address| address | OCCUPIED),
        );
        Some(Reservation {
            key,
            owner: marker::PhantomData,
        })
    }
}

impl<T: Copy> Drop for Slots<'_, T> {
    fn drop(&mut self) {
        // SAFETY: ArenaOwner keeps the group allocated while Slots is live.
        let group = unsafe { self.group.as_ref() };
        debug_assert!(group.leased.get());
        group.leased.set(false);
    }
}

impl<'owner, T: Copy> Reservation<'owner, T> {
    pub fn key(&self) -> Key<'owner, T> {
        Key {
            echo: self.key,
            owner: marker::PhantomData,
        }
    }

    pub fn commit(self) -> Lease<'owner, T> {
        let this = mem::ManuallyDrop::new(self);
        Lease {
            key: this.key,
            owner: marker::PhantomData,
        }
    }
}

impl<T: Copy> Drop for Reservation<'_, T> {
    fn drop(&mut self) {
        self.key.release();
    }
}

impl<T: Copy> Lease<'_, T> {
    pub fn key(&self) -> Key<'_, T> {
        Key {
            echo: self.key,
            owner: marker::PhantomData,
        }
    }

    pub fn value(&self) -> T {
        self.key.value()
    }

    pub fn complete(self) -> T {
        let this = mem::ManuallyDrop::new(self);
        let value = this.key.value();
        // SAFETY: a live lease always refers to an occupied entry.
        let state = unsafe { this.key.0.as_ref() }.state.get();
        if state.addr() & TERMINAL == 0 {
            process::abort();
        }
        this.key.release();
        value
    }
}

impl<T: Copy> Drop for Lease<'_, T> {
    fn drop(&mut self) {
        // SAFETY: ArenaOwner keeps the entry allocated for the lease lifetime.
        let entry = unsafe { self.key.0.as_ref() };
        let state = entry.state.get();
        if state.addr() & OCCUPIED == 0 {
            process::abort();
        }
        if state.addr() & TERMINAL != 0 {
            self.key.release();
        } else {
            entry
                .state
                .set(state.map_addr(|address| address | DETACHED));
        }
    }
}

impl<T: Copy> Key<'_, T> {
    /// Exposes the key as an address-sized integer for an external producer.
    pub fn expose(self) -> usize {
        self.echo.expose()
    }

    /// Erases the arena-owner lifetime without converting the key to an
    /// integer.
    ///
    /// # Safety
    ///
    /// The arena must remain alive until the returned echo reaches terminal
    /// completion, even if every lifetime-bearing handle is dropped first. A
    /// terminal completion may be resolved exactly once, and no copy of the
    /// echo may be used after that resolution.
    pub unsafe fn erase(self) -> Echo<T> {
        self.echo
    }
}

impl<'arena, T: Copy> Drain<'arena, T> {
    pub fn complete(&self, key: Echo<T>) -> Completion<'_, T> {
        Completion {
            key,
            arena: marker::PhantomData,
        }
    }
}

impl<T: Copy> Completion<'_, T> {
    pub fn resolve(self, more: bool) -> T {
        let value = self.key.value();
        self.key.complete_external(more);
        value
    }
}

impl<T: Copy> Echo<T> {
    /// Exposes the token as an address-sized integer for an external producer.
    pub fn expose(self) -> usize {
        self.0.as_ptr().expose_provenance()
    }

    /// Reconstructs a token returned by an external producer.
    ///
    /// # Safety
    ///
    /// `address` must have been produced by [`expose`](Self::expose) for a
    /// currently occupied entry in the arena used to resolve it. A terminal
    /// completion may be resolved exactly once, after which every token for
    /// this address is invalid.
    pub unsafe fn from_exposed(address: usize) -> Option<Self> {
        if address == 0 || address & (mem::align_of::<Entry<T>>() - 1) != 0 {
            return None;
        }
        Some(Self(unsafe {
            ptr::NonNull::new_unchecked(ptr::with_exposed_provenance_mut(address))
        }))
    }

    fn value(self) -> T {
        // SAFETY: every externally visible Echo refers to an occupied entry,
        // whose value is initialized before the token is issued.
        unsafe { self.0.as_ref().value.get().assume_init() }
    }

    fn complete_external(self, more: bool) {
        if more {
            return;
        }
        // SAFETY: ArenaOwner keeps the entry allocated through completion.
        let entry = unsafe { self.0.as_ref() };
        let state = entry.state.get();
        if state.addr() & OCCUPIED == 0 {
            process::abort();
        }
        entry
            .state
            .set(state.map_addr(|address| address | TERMINAL));
        if state.addr() & DETACHED != 0 {
            self.release();
        }
    }

    fn release(self) {
        // SAFETY: a valid occupied entry retains its owning group pointer in
        // the unflagged state bits.
        let entry = unsafe { self.0.as_ref() };
        let state = entry.state.get();
        if state.addr() & OCCUPIED == 0 {
            process::abort();
        }
        let owner = state
            .map_addr(|address| address & !FLAGS)
            .cast::<Group<T>>();
        // SAFETY: ArenaOwner keeps every group live until terminal completion.
        let group = unsafe { &*owner };
        entry.state.set(group.free.get().cast());
        group.free.set(self.0.as_ptr());
    }
}

impl<T: Copy> Clone for Echo<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for Echo<T> {}

impl<T: Copy> std::fmt::Debug for Echo<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Echo").field(&self.0).finish()
    }
}

impl<T: Copy> PartialEq for Echo<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl<T: Copy> Eq for Echo<T> {}

impl<T: Copy> Clone for Key<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for Key<'_, T> {}

impl<T: Copy> std::fmt::Debug for Key<'_, T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_tuple("Key").field(&self.echo).finish()
    }
}

impl<T: Copy> PartialEq for Key<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.echo == other.echo
    }
}

impl<T: Copy> Eq for Key<'_, T> {}

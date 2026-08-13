use std::{fmt, marker, mem, process, ptr};

use crate::collections::{self, completion};

mod group;

const OCCUPIED: usize = 1;
const DETACHED: usize = 2;
const TERMINAL: usize = 4;
const FLAGS: usize = OCCUPIED | DETACHED | TERMINAL;

type Invariant<'owner> = marker::PhantomData<fn(&'owner ()) -> &'owner ()>;

/// Reusable externally completed entries held live by an owner proof.
pub struct Arena<T: Copy> {
    groups: Option<ptr::NonNull<group::Group<T>>>,
    owner: marker::PhantomData<Box<group::Group<T>>>,
}

/// A leased group of reusable completion entries.
#[repr(transparent)]
pub struct Slots<'owner, T: Copy> {
    group: ptr::NonNull<group::Group<T>>,
    owner: Invariant<'owner>,
}

/// A completion entry that rolls back unless committed.
#[must_use = "a reserved completion must be committed or released"]
#[repr(transparent)]
pub struct Reservation<'owner, T: Copy> {
    key: Token<T>,
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
    key: Token<T>,
    owner: Invariant<'owner>,
}

/// A pointer-sized completion key tied to its arena owner.
#[repr(transparent)]
pub struct Key<'owner, T: Copy> {
    echo: Token<T>,
    owner: Invariant<'owner>,
}

/// A checked identity echoed by an external completion source.
#[repr(C)]
pub struct Token<T: Copy> {
    pointer: ptr::NonNull<group::Entry<T>>,
    serial: u64,
}

/// Borrowed authority to resolve echoed completion tokens.
pub struct Drain<'arena, T: Copy> {
    arena: &'arena Arena<T>,
}

/// An echoed token whose resolution is tied to a live arena borrow.
#[repr(transparent)]
pub struct Resolved<'arena, T: Copy> {
    key: Token<T>,
    arena: marker::PhantomData<&'arena mut Arena<T>>,
}

const _: () = {
    assert!(mem::align_of::<group::Group<usize>>() >= 8);
    assert!(mem::size_of::<group::Entry<usize>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Token<usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Key<'static, usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<'static, usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Lease<'static, usize>>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Slots<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Resolved<'static, usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Drain<'static, usize>>() == mem::size_of::<usize>());
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
        owner: impl completion::raw::ArenaOwner<'owner, T>,
        capacity: usize,
    ) -> Result<Slots<'owner, T>, collections::AllocationError> {
        let mut arena = owner.arena();
        // SAFETY: ArenaOwner provides exclusive access for this call and keeps
        // the arena alive for every handle and external echo issued here.
        unsafe { arena.as_mut() }.try_slots_inner(capacity)
    }

    /// Borrows authority to resolve external completions.
    pub fn drain(&self) -> Drain<'_, T> {
        Drain { arena: self }
    }

    fn try_slots_inner<'owner>(
        &mut self,
        capacity: usize,
    ) -> Result<Slots<'owner, T>, collections::AllocationError> {
        let mut selected = None;
        let mut retained = None;
        let mut quiescent = None;
        let mut cursor = self.groups.take();
        while let Some(group) = group::Group::pop(&mut cursor) {
            // SAFETY: every pointer in the arena list owns one live group.
            let group_ref = unsafe { group.as_ref() };
            if !group_ref.is_quiescent() {
                group::Group::push(&mut retained, group);
                continue;
            }
            let len = group_ref.entries.len();
            let better = selected.is_none_or(|selected: ptr::NonNull<group::Group<T>>| {
                // SAFETY: selected owns a live group detached from every list.
                len < unsafe { selected.as_ref() }.entries.len()
            });
            if len >= capacity && better {
                if let Some(previous) = selected.replace(group) {
                    group::Group::push(&mut quiescent, previous);
                }
            } else {
                group::Group::push(&mut quiescent, group);
            }
        }

        let group = match selected {
            Some(group) => group,
            None => match group::Group::try_new(capacity) {
                Ok(group) => group,
                Err(error) => {
                    self.groups = retained;
                    while let Some(group) = group::Group::pop(&mut quiescent) {
                        group::Group::push(&mut self.groups, group);
                    }
                    return Err(error);
                }
            },
        };
        while let Some(group) = group::Group::pop(&mut quiescent) {
            // SAFETY: quiescent groups have no live slot or entry pointers.
            unsafe { group::Group::drop_owned(group) };
        }

        // SAFETY: group owns a live allocation detached from every list.
        unsafe { group.as_ref() }.prepare(capacity);
        group::Group::push(&mut retained, group);
        self.groups = retained;
        Ok(Slots {
            group,
            owner: marker::PhantomData,
        })
    }
}

impl<T: Copy> Drop for Arena<T> {
    fn drop(&mut self) {
        while let Some(group) = group::Group::pop(&mut self.groups) {
            // SAFETY: ArenaOwner requires every issued handle and external
            // echo to finish before the arena is dropped.
            unsafe { group::Group::drop_owned(group) };
        }
    }
}

impl<T: Copy> Default for Arena<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'owner, T: Copy> Slots<'owner, T> {
    pub fn reserve(&self, value: T) -> Option<Reservation<'owner, T>> {
        let owner = self.group;
        // SAFETY: ArenaOwner keeps the group allocated while Slots is live.
        let group = unsafe { owner.as_ref() };
        debug_assert!(group.leased.get());
        let pointer = ptr::NonNull::new(group.free.get())?;
        let serial = group.serial.get().wrapping_add(1).max(1);
        group.serial.set(serial);
        let key = Token { pointer, serial };
        // SAFETY: free only contains entries owned by this group.
        let entry = unsafe { key.pointer.as_ref() };
        let next = entry.state.get().cast::<group::Entry<T>>();
        debug_assert_eq!(next.addr() & OCCUPIED, 0);
        group.free.set(next);
        entry.value.set(mem::MaybeUninit::new(value));
        entry.serial.set(serial);
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
        let state = unsafe { this.key.pointer.as_ref() }.state.get();
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
        let entry = unsafe { self.key.pointer.as_ref() };
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
    /// Creates a copyable token for an external producer.
    pub fn token(self) -> Token<T> {
        self.echo
    }

    pub fn expose(self) -> (usize, u64) {
        (self.echo.address(), self.echo.serial())
    }
}

impl<'arena, T: Copy> Drain<'arena, T> {
    pub fn complete(&mut self, key: Token<T>) -> Option<Resolved<'_, T>> {
        let mut cursor = self.arena.groups;
        while let Some(group) = cursor {
            let group = unsafe { group.as_ref() };
            for entry in group.entries.iter() {
                if ptr::from_ref(entry) != key.pointer.as_ptr() {
                    continue;
                }
                let occupied = entry.state.get().addr() & OCCUPIED != 0;
                if occupied && entry.serial.get() == key.serial {
                    return Some(Resolved {
                        key,
                        arena: marker::PhantomData,
                    });
                }
                return None;
            }
            cursor = group.next.get();
        }
        None
    }
}

impl<T: Copy> Resolved<'_, T> {
    pub fn resolve(self, more: bool) -> T {
        let value = self.key.value();
        self.key.complete_external(more);
        value
    }
}

impl<T: Copy> Token<T> {
    pub fn address(self) -> usize {
        self.pointer.as_ptr().expose_provenance()
    }

    pub fn serial(self) -> u64 {
        self.serial
    }

    /// Reconstructs inert identity data returned by an external producer.
    pub fn from_parts(address: usize, serial: u64) -> Option<Self> {
        if address == 0 || address & (mem::align_of::<group::Entry<T>>() - 1) != 0 {
            return None;
        }
        Some(Self {
            pointer: ptr::NonNull::new(ptr::with_exposed_provenance_mut(address))?,
            serial,
        })
    }

    fn value(self) -> T {
        // SAFETY: every externally visible Token refers to an occupied entry,
        // whose value is initialized before the token is issued.
        unsafe { self.pointer.as_ref().value.get().assume_init() }
    }

    fn complete_external(self, more: bool) {
        if more {
            return;
        }
        // SAFETY: ArenaOwner keeps the entry allocated through completion.
        let entry = unsafe { self.pointer.as_ref() };
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
        let entry = unsafe { self.pointer.as_ref() };
        let state = entry.state.get();
        if state.addr() & OCCUPIED == 0 {
            process::abort();
        }
        let owner = state
            .map_addr(|address| address & !FLAGS)
            .cast::<group::Group<T>>();
        // SAFETY: ArenaOwner keeps every group live until terminal completion.
        let group = unsafe { &*owner };
        entry.state.set(group.free.get().cast());
        group.free.set(self.pointer.as_ptr());
    }
}

impl<T: Copy> Clone for Token<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for Token<T> {}

impl<T: Copy> fmt::Debug for Token<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Token")
            .field("address", &self.address())
            .field("serial", &self.serial)
            .finish()
    }
}

impl<T: Copy> PartialEq for Token<T> {
    fn eq(&self, other: &Self) -> bool {
        self.pointer == other.pointer && self.serial == other.serial
    }
}

impl<T: Copy> Eq for Token<T> {}

impl<T: Copy> Clone for Key<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T: Copy> Copy for Key<'_, T> {}

impl<T: Copy> fmt::Debug for Key<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_tuple("Key").field(&self.echo).finish()
    }
}

impl<T: Copy> PartialEq for Key<'_, T> {
    fn eq(&self, other: &Self) -> bool {
        self.echo == other.echo
    }
}

impl<T: Copy> Eq for Key<'_, T> {}

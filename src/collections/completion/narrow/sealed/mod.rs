use std::{marker, mem, process, ptr};

use crate::collections::{self, completion::narrow};

mod group;
mod identities;

pub use identities::{Echo, Key};

const OCCUPIED: usize = 1;
const DETACHED: usize = 2;
const TERMINAL: usize = 4;
const FLAGS: usize = OCCUPIED | DETACHED | TERMINAL;

type Invariant<'owner> = marker::PhantomData<fn(&'owner ()) -> &'owner ()>;

/// Reusable entries with configurable one-word identities.
///
/// Index bits select entries; generation bits reject stale echoes after reuse.
pub struct Arena<T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    groups: Option<ptr::NonNull<group::Group<T>>>,
    directory: Vec<ptr::NonNull<group::Entry<T>>>,
    owner: marker::PhantomData<Box<group::Group<T>>>,
}

/// A leased group of reusable narrow completion entries.
#[repr(transparent)]
pub struct Slots<'owner, T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    group: ptr::NonNull<group::Group<T>>,
    owner: Invariant<'owner>,
}

/// A narrow completion entry that rolls back unless committed.
#[must_use = "a reserved completion must be committed or released"]
#[repr(transparent)]
pub struct Reservation<'owner, T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32>
{
    entry: ptr::NonNull<group::Entry<T>>,
    owner: Invariant<'owner>,
}

/// The Rust owner's one-word claim on an externally completing entry.
///
/// The lifetime is invariant and cannot be widened independently of the arena
/// owner.
///
/// ```compile_fail
/// use o3::collections::completion::narrow::Lease;
///
/// fn widen<'short, 'long, T: Copy>(lease: Lease<'short, T>) -> Lease<'long, T> {
///     lease
/// }
/// ```
#[must_use = "a live completion must reach terminal completion or be detached"]
#[repr(transparent)]
pub struct Lease<'owner, T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    entry: ptr::NonNull<group::Entry<T>>,
    owner: Invariant<'owner>,
}

/// Borrowed authority to resolve narrow external completion echoes.
pub struct Drain<'arena, T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    arena: &'arena Arena<T, INDEX_BITS, GENERATION_BITS>,
}

/// A validated narrow echo whose resolution is tied to a live arena borrow.
/// Resolution rechecks its generation, so multiple decoded completions can be
/// retained without allowing one to resolve a later reuse of the same entry.
#[repr(C)]
pub struct Resolved<'arena, T: Copy, const INDEX_BITS: u32 = 32, const GENERATION_BITS: u32 = 32> {
    entry: ptr::NonNull<group::Entry<T>>,
    generation: u32,
    arena: marker::PhantomData<&'arena Arena<T, INDEX_BITS, GENERATION_BITS>>,
}

const _: () = {
    assert!(mem::align_of::<group::Group<usize>>() >= 8);
    assert!(mem::size_of::<group::Entry<usize>>() == 3 * mem::size_of::<usize>());
    assert!(mem::size_of::<Echo<usize>>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Option<Echo<usize>>>() == mem::size_of::<u64>());
    assert!(mem::size_of::<Key<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<Lease<'static, usize>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Slots<'static, usize>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Resolved<'static, usize>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Drain<'static, usize>>() == mem::size_of::<usize>());
};

const fn validate_widths<const INDEX_BITS: u32, const GENERATION_BITS: u32>() {
    assert!(INDEX_BITS != 0, "narrow completion requires index bits");
    assert!(
        INDEX_BITS <= u32::BITS,
        "narrow completion index exceeds u32"
    );
    assert!(
        GENERATION_BITS != 0,
        "narrow completion requires generation bits"
    );
    assert!(
        GENERATION_BITS <= u32::BITS,
        "narrow completion generation exceeds u32"
    );
    assert!(
        INDEX_BITS + GENERATION_BITS <= u64::BITS,
        "narrow completion exceeds u64"
    );
}

const fn generation_max<const GENERATION_BITS: u32>() -> u32 {
    if GENERATION_BITS == u32::BITS {
        u32::MAX
    } else {
        (1u32 << GENERATION_BITS) - 1
    }
}

const fn index_limit<const INDEX_BITS: u32>() -> usize {
    if INDEX_BITS == u32::BITS {
        u32::MAX as usize
    } else {
        1usize << INDEX_BITS
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Arena<T, INDEX_BITS, GENERATION_BITS>
{
    pub const fn new() -> Self {
        validate_widths::<INDEX_BITS, GENERATION_BITS>();
        Self {
            groups: None,
            directory: Vec::new(),
            owner: marker::PhantomData,
        }
    }

    /// Acquires a reusable group using an external owner lifetime.
    pub fn try_slots<'owner>(
        owner: impl narrow::raw::ArenaOwner<'owner, T, INDEX_BITS, GENERATION_BITS>,
        capacity: usize,
    ) -> Result<Slots<'owner, T, INDEX_BITS, GENERATION_BITS>, collections::AllocationError> {
        let mut arena = owner.arena();
        // SAFETY: ArenaOwner provides exclusive access for this call and keeps
        // the arena alive for every handle and external echo issued here.
        unsafe { arena.as_mut() }.try_slots_inner(capacity)
    }

    /// Borrows authority to resolve external completion echoes.
    pub fn drain(&self) -> Drain<'_, T, INDEX_BITS, GENERATION_BITS> {
        Drain { arena: self }
    }

    fn try_slots_inner<'owner>(
        &mut self,
        capacity: usize,
    ) -> Result<Slots<'owner, T, INDEX_BITS, GENERATION_BITS>, collections::AllocationError> {
        let generation_max = generation_max::<GENERATION_BITS>();
        let mut selected = None;
        let mut cursor = self.groups;
        while let Some(group) = cursor {
            // SAFETY: every pointer in the arena list owns one live group.
            let group_ref = unsafe { group.as_ref() };
            if group_ref.is_quiescent() {
                let available = group_ref.available(generation_max);
                let better = selected.is_none_or(
                    |(_, selected_available): (ptr::NonNull<group::Group<T>>, usize)| {
                        available < selected_available
                    },
                );
                if available >= capacity && better {
                    selected = Some((group, available));
                }
            }
            cursor = group_ref.next.get();
        }

        let group = match selected {
            Some((group, _)) => group,
            None => self.try_new_group(capacity)?,
        };
        // SAFETY: the selected group is quiescent or freshly allocated and is
        // owned by this arena.
        unsafe { group.as_ref() }.prepare(capacity, generation_max);
        Ok(Slots {
            group,
            owner: marker::PhantomData,
        })
    }

    fn try_new_group(
        &mut self,
        capacity: usize,
    ) -> Result<ptr::NonNull<group::Group<T>>, collections::AllocationError> {
        let required = self
            .directory
            .len()
            .checked_add(capacity)
            .ok_or_else(collections::AllocationError::overflow)?;
        if required > index_limit::<INDEX_BITS>() {
            return Err(collections::AllocationError::overflow());
        }
        self.directory.try_reserve_exact(capacity).map_err(|_| {
            collections::AllocationError::for_array::<ptr::NonNull<group::Entry<T>>>(required)
        })?;
        let base = self.directory.len() as u32;
        let group = group::Group::try_new(capacity, base)?;
        // SAFETY: the fresh group owns an initialized, address-stable slice.
        let entries = &unsafe { group.as_ref() }.entries;
        self.directory
            .extend(entries.iter().map(ptr::NonNull::from));
        group::Group::push(&mut self.groups, group);
        Ok(group)
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Drop
    for Arena<T, INDEX_BITS, GENERATION_BITS>
{
    fn drop(&mut self) {
        while let Some(group) = group::Group::pop(&mut self.groups) {
            // SAFETY: ArenaOwner requires every issued handle and external
            // echo to finish before the arena is dropped.
            unsafe { group::Group::drop_owned(group) };
        }
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Default
    for Arena<T, INDEX_BITS, GENERATION_BITS>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<'owner, T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Slots<'owner, T, INDEX_BITS, GENERATION_BITS>
{
    pub fn reserve(&self, value: T) -> Option<Reservation<'owner, T, INDEX_BITS, GENERATION_BITS>> {
        // SAFETY: ArenaOwner keeps the group allocated while Slots is live.
        let group = unsafe { self.group.as_ref() };
        debug_assert!(group.leased.get());
        let entry = ptr::NonNull::new(group.free.get())?;
        // SAFETY: free only contains entries owned by this group.
        let entry_ref = unsafe { entry.as_ref() };
        let next = entry_ref.state.get().cast::<group::Entry<T>>();
        debug_assert_eq!(next.addr() & OCCUPIED, 0);
        let generation = entry_ref.generation.get().checked_add(1)?;
        debug_assert!(generation <= generation_max::<GENERATION_BITS>());
        group.free.set(next);
        entry_ref.value.set(mem::MaybeUninit::new(value));
        entry_ref.generation.set(generation);
        entry_ref.state.set(
            self.group
                .as_ptr()
                .cast::<()>()
                .map_addr(|address| address | OCCUPIED),
        );
        Some(Reservation {
            entry,
            owner: marker::PhantomData,
        })
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Drop
    for Slots<'_, T, INDEX_BITS, GENERATION_BITS>
{
    fn drop(&mut self) {
        // SAFETY: ArenaOwner keeps the group allocated while Slots is live.
        let group = unsafe { self.group.as_ref() };
        debug_assert!(group.leased.get());
        group.leased.set(false);
    }
}

impl<'owner, T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Reservation<'owner, T, INDEX_BITS, GENERATION_BITS>
{
    pub fn key(&self) -> Key<'owner, T, INDEX_BITS, GENERATION_BITS> {
        Key {
            echo: Echo::from_entry(self.entry),
            owner: marker::PhantomData,
        }
    }

    pub fn commit(self) -> Lease<'owner, T, INDEX_BITS, GENERATION_BITS> {
        let this = mem::ManuallyDrop::new(self);
        Lease {
            entry: this.entry,
            owner: marker::PhantomData,
        }
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Drop
    for Reservation<'_, T, INDEX_BITS, GENERATION_BITS>
{
    fn drop(&mut self) {
        release(self.entry, generation_max::<GENERATION_BITS>());
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Lease<'_, T, INDEX_BITS, GENERATION_BITS>
{
    pub fn key(&self) -> Key<'_, T, INDEX_BITS, GENERATION_BITS> {
        Key {
            echo: Echo::from_entry(self.entry),
            owner: marker::PhantomData,
        }
    }

    pub fn value(&self) -> T {
        value(self.entry)
    }

    pub fn complete(self) -> T {
        let this = mem::ManuallyDrop::new(self);
        let value = value(this.entry);
        // SAFETY: a live lease always refers to an occupied entry.
        let state = unsafe { this.entry.as_ref() }.state.get();
        if state.addr() & TERMINAL == 0 {
            process::abort();
        }
        release(this.entry, generation_max::<GENERATION_BITS>());
        value
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32> Drop
    for Lease<'_, T, INDEX_BITS, GENERATION_BITS>
{
    fn drop(&mut self) {
        // SAFETY: ArenaOwner keeps the entry allocated for the lease lifetime.
        let entry = unsafe { self.entry.as_ref() };
        let state = entry.state.get();
        if state.addr() & OCCUPIED == 0 {
            process::abort();
        }
        if state.addr() & TERMINAL != 0 {
            release(self.entry, generation_max::<GENERATION_BITS>());
        } else {
            entry
                .state
                .set(state.map_addr(|address| address | DETACHED));
        }
    }
}

impl<'arena, T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Drain<'arena, T, INDEX_BITS, GENERATION_BITS>
{
    pub fn complete(
        &self,
        echo: Echo<T, INDEX_BITS, GENERATION_BITS>,
    ) -> Option<Resolved<'arena, T, INDEX_BITS, GENERATION_BITS>> {
        let entry = *self.arena.directory.get(echo.index() as usize)?;
        // SAFETY: every directory pointer names an entry owned by this arena.
        let entry_ref = unsafe { entry.as_ref() };
        let occupied = entry_ref.state.get().addr() & OCCUPIED != 0;
        if !occupied || entry_ref.generation.get() != echo.generation() {
            return None;
        }
        Some(Resolved {
            entry,
            generation: echo.generation(),
            arena: marker::PhantomData,
        })
    }
}

impl<T: Copy, const INDEX_BITS: u32, const GENERATION_BITS: u32>
    Resolved<'_, T, INDEX_BITS, GENERATION_BITS>
{
    pub fn resolve(self, more: bool) -> Option<T> {
        // SAFETY: the arena lifetime keeps every directory entry allocated.
        let entry = unsafe { self.entry.as_ref() };
        let occupied = entry.state.get().addr() & OCCUPIED != 0;
        if !occupied || entry.generation.get() != self.generation {
            return None;
        }
        let value = value(self.entry);
        complete_external(self.entry, more, generation_max::<GENERATION_BITS>());
        Some(value)
    }
}

fn value<T: Copy>(entry: ptr::NonNull<group::Entry<T>>) -> T {
    // SAFETY: every externally visible identity refers to an occupied entry,
    // whose value is initialized before the identity is issued.
    unsafe { entry.as_ref().value.get().assume_init() }
}

fn complete_external<T: Copy>(
    entry: ptr::NonNull<group::Entry<T>>,
    more: bool,
    generation_max: u32,
) {
    if more {
        return;
    }
    // SAFETY: ArenaOwner keeps the entry allocated through completion.
    let entry_ref = unsafe { entry.as_ref() };
    let state = entry_ref.state.get();
    if state.addr() & OCCUPIED == 0 {
        process::abort();
    }
    entry_ref
        .state
        .set(state.map_addr(|address| address | TERMINAL));
    if state.addr() & DETACHED != 0 {
        release(entry, generation_max);
    }
}

fn release<T: Copy>(entry: ptr::NonNull<group::Entry<T>>, generation_max: u32) {
    // SAFETY: a valid occupied entry retains its owning group pointer in the
    // unflagged state bits.
    let entry_ref = unsafe { entry.as_ref() };
    let state = entry_ref.state.get();
    if state.addr() & OCCUPIED == 0 {
        process::abort();
    }
    let owner = state
        .map_addr(|address| address & !FLAGS)
        .cast::<group::Group<T>>();
    if entry_ref.generation.get() == generation_max {
        entry_ref.state.set(ptr::null_mut());
        return;
    }
    // SAFETY: ArenaOwner keeps every group live until terminal completion.
    let group = unsafe { &*owner };
    entry_ref.state.set(group.free.get().cast());
    group.free.set(entry.as_ptr());
}

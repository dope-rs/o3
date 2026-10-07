use std::{cell, mem, ptr};

use pool::state;

use crate::{
    buffer,
    buffer::{
        pool,
        pool::owners::{Free, Key},
        resident,
        storage::{backing, regions},
    },
    mem::rc,
};

#[repr(transparent)]
#[derive(Debug)]
struct Prefix(regions::Prefix);

impl Prefix {
    const fn vacant(next: Option<Key<true>>) -> Self {
        Self(regions::Prefix::empty(Key::<true>::encode(next)))
    }

    /// # Safety
    /// This prefix must be one vacant account cell exclusively owned by the
    /// caller, and `slot` the exact pool slot key linked to it.
    unsafe fn activate(&self, slot: super::Key) {
        use crate::mem::raw::Edge;

        // SAFETY: the caller owns this vacant cell, so its prefix word is
        // exclusively writable and holds no live edge yet.
        unsafe { self.0.write(slot.get()) };
        // SAFETY: the linked slot key is complete, so the first external
        // account edge may now be published.
        unsafe { self.0.activate() };
    }

    unsafe fn slot(&self) -> super::Key {
        // SAFETY: an active or retired Account stores the exact linked slot key.
        unsafe { super::Key::new(self.0.word()) }
    }

    /// # Safety
    /// This prefix must be one terminally retired account cell exclusively
    /// owned by the caller, with no live edge.
    unsafe fn recycle(&self, next: Option<Key<true>>) {
        // SAFETY: terminal account ownership leaves this cell without edges and
        // exclusively writable before its vacant successor is published.
        unsafe { self.0.write(Key::<true>::encode(next)) };
    }
}

#[repr(C, align(8))]
pub(super) struct Accounts<'d> {
    root: mem::ManuallyDrop<resident::Charge<'d>>,
    free: Free<true>,
}

/// Validated current account free-list head before its infallible activation.
struct Claim<'a, 'd> {
    table: &'a Accounts<'d>,
    account: &'a Account<'d>,
    key: Key<true>,
}

#[repr(transparent)]
struct Active<'a, 'd>(&'a Account<'d>);

#[repr(transparent)]
struct Retired<'d>(ptr::NonNull<Account<'d>>);

impl<'a, 'd> Claim<'a, 'd> {
    fn new(entry: super::Entry<'a, super::Resident<'d>>) -> Option<Self> {
        // SAFETY: the live resident entry retains its initialized accounting table.
        let table = unsafe { Accounts::ptr(entry.core()).as_ref() };
        let key = table.free.head()?;
        // SAFETY: this key comes from the free list of the entry's exact table.
        let account = unsafe { Accounts::entry(entry, key) };
        Some(Self {
            table,
            account,
            key,
        })
    }

    fn activate(
        self,
        slot: super::Active<super::Resident<'d>, state::Uninit>,
        charge: resident::Charge<'d>,
    ) -> Registered<'d> {
        let account = self.account;
        let ptr = ptr::NonNull::from(account);
        // SAFETY: Claim proves this vacant prefix contains only its exact
        // Accounts free-list successor.
        unsafe { self.table.free.pop(account.prefix.0.word()) };
        // SAFETY: the claimed Account's resident cell is uninitialized and
        // exclusively owned by this unpublished transition.
        unsafe { *account.resident.get() = mem::MaybeUninit::new(charge) };
        // SAFETY: Claim owns the vacant Account configuration, and both the
        // exact slot identity and resident domain are initialized here.
        let slot = mem::ManuallyDrop::new(slot);
        unsafe { account.prefix.activate(slot.key) };
        // SAFETY: activation created the first external account edge.
        unsafe { Registered::new(ptr) }
    }
}

impl<'a, 'd> Active<'a, 'd> {
    fn try_retain(
        self,
        budget: resident::Budget<'d>,
    ) -> Result<Option<Registered<'d>>, resident::AllocError> {
        use crate::mem::raw::Edge;

        // SAFETY: a linked Account initialized its resident domain before
        // publication and retains it until terminal release.
        let charge = unsafe { (*self.0.resident.get()).assume_init_ref() };
        if !ptr::eq(budget.core(), charge.budget().core()) {
            return Ok(None);
        }
        // SAFETY: this active account can issue one additional external edge.
        unsafe { self.0.prefix.0.try_retain() }.map_err(resident::AllocError::Retain)?;
        // SAFETY: the retained edge is branded by the checked budget domain.
        Ok(Some(unsafe { Registered::new(ptr::NonNull::from(self.0)) }))
    }
}

impl<'d> Retired<'d> {
    unsafe fn new(ptr: ptr::NonNull<Account<'d>>) -> Self {
        Self(ptr)
    }

    fn account(&self) -> &Account<'d> {
        // SAFETY: Retired owns the terminal Account until finish completes.
        unsafe { self.0.as_ref() }
    }

    fn finish(self) {
        let account = self.account();
        // SAFETY: Retired preserves the active Account's exact linked slot.
        let slot = unsafe { account.prefix.slot() };
        let core = account.core();
        let table = Accounts::ptr(core);
        // SAFETY: this terminal account is one published table cell.
        let first = unsafe { Accounts::cells(table.as_ptr()) };
        // SAFETY: this terminal account belongs to the same bounded u32 array.
        let index = unsafe { self.0.as_ptr().offset_from_unsigned(first) };
        let key = unsafe { Key::<true>::new(index as u32) };
        // SAFETY: terminal Account ownership keeps its accounted table live.
        let table = unsafe { table.as_ref() };
        // SAFETY: activation initialized the resident domain and terminal
        // ownership consumes it exactly once before publishing this vacant account.
        unsafe { (*account.resident.get()).assume_init_drop() };
        let slots = core;
        // SAFETY: this terminal Account is the exact link published in `slot`.
        unsafe { slots.ptr(slot).as_ref().unlink() };
        table.recycle(key, self);
        // SAFETY: this retired Account transfers the slot edge retained at activation.
        unsafe { super::Active::release(slots, slot) };
    }
}

impl<'d> Accounts<'d> {
    pub(super) unsafe fn initialize(
        base: super::Base<super::Accounted>,
        slots: u32,
        root: resident::Charge<'d>,
        limit: u32,
    ) -> super::Base<super::Resident<'d>> {
        // SAFETY: the accounted geometry is unchanged; this initialization
        // transfers the exact domain charge into its resident account table.
        let base = unsafe { super::Base::from_raw(base.as_raw().cast()) };
        // SAFETY: the accounted Block reserves its Accounts table immediately
        // after exactly `slots` Slot cells.
        let table = unsafe { base.accounts(slots) };
        // SAFETY: `table` points to aligned, allocated, uninitialized storage
        // reserved for exactly one Accounts header.
        unsafe {
            table.write(Self {
                root: mem::ManuallyDrop::new(root),
                free: Free::new(limit),
            });
        }
        // SAFETY: the admitted layout reserves exactly `limit` trailing cells.
        let entries = unsafe { Self::cells(table.as_ptr()) }.cast_mut();
        for index in 0..limit {
            let next = if index + 1 == limit {
                None
            } else {
                // SAFETY: index + 1 remains within this exact Account array.
                Some(unsafe { Key::<true>::new(index + 1) })
            };
            // SAFETY: every index is inside that Account array and is written
            // exactly once before the pool is published.
            unsafe { entries.add(index as usize).write(Account::new(base, next)) };
        }
        base
    }

    fn ptr<S>(core: super::Ptr<super::Resident<'d>, S>) -> ptr::NonNull<Self> {
        let header = core.header();
        // SAFETY: a published accounted core was initialized from the matching
        // typed layout and carries its immutable Slot count.
        unsafe { core.base().accounts(header.slots()) }
    }

    /// Start of the exact `Account` array reserved directly after this header.
    ///
    /// # Safety
    /// `table` points at an aligned `Accounts` header and its admitted `Account` storage.
    unsafe fn cells(table: *const Self) -> *const Account<'d> {
        // SAFETY: Layout places every Account cell directly after the Accounts
        // header inside this same accounted allocation.
        unsafe { table.add(1).cast::<Account<'d>>() }
    }

    unsafe fn entry<'a>(
        owner: super::Entry<'a, super::Resident<'d>>,
        key: Key<true>,
    ) -> &'a Account<'d> {
        let table = Self::ptr(owner.core());
        // SAFETY: the typed key originates from this exact initialized table,
        // and the original entry borrow retains the allocation for 'a.
        unsafe { &*Self::cells(table.as_ptr()).add(key.get() as usize) }
    }

    fn recycle(&self, key: Key<true>, retired: Retired<'d>) {
        // SAFETY: Retired owns this exact account and writes its vacant link
        // before the common head publishes the returned key.
        unsafe {
            self.free
                .push(key, |next| retired.account().prefix.recycle(next))
        };
    }

    pub(super) unsafe fn release<S>(core: super::Ptr<super::Resident<'d>, S>) {
        // SAFETY: the accounted header owns one initialized Accounts table.
        let table = unsafe { Self::ptr(core).as_mut() };
        // SAFETY: terminal allocation ownership returns its retained charge once.
        unsafe { mem::ManuallyDrop::drop(&mut table.root) };
    }
}

#[repr(C, align(8))]
#[derive(Debug)]
pub(in crate::buffer) struct Account<'d> {
    prefix: Prefix,
    core: super::Base<super::Resident<'d>>,
    resident: cell::UnsafeCell<mem::MaybeUninit<resident::Charge<'d>>>,
}

impl<'d> Account<'d> {
    const fn new(core: super::Base<super::Resident<'d>>, next: Option<Key<true>>) -> Self {
        Self {
            prefix: Prefix::vacant(next),
            core,
            resident: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
        }
    }

    fn core(&self) -> super::Ptr<super::Resident<'d>, super::Any> {
        // SAFETY: Account cells are reachable only after their Block completed
        // every metadata write and published the resident pool. Slot rooting
        // keeps that allocation live through account terminal release.
        unsafe { self.core.publish() }
    }

    pub(in crate::buffer) fn try_bind<M>(
        lease: &pool::Span<super::Resident<'d>, M>,
        budget: &resident::Budget<'d>,
    ) -> Result<Option<Registered<'d>>, resident::AllocError> {
        let span = lease;
        let entry = span.entry();
        let link = match entry.association() {
            Ok(key) => {
                // SAFETY: this entry's slot link was published after its account activated.
                let active = Active(unsafe { Accounts::entry(entry, key) });
                return active.try_retain(*budget);
            }
            Err(link) => link,
        };
        let Some(claim) = Claim::new(entry) else {
            return Ok(None);
        };
        let charge = budget
            .try_charge(entry.capacity().get() as usize)
            .map_err(resident::AllocError::Budget)?;
        let entry = entry.try_retain().map_err(resident::AllocError::Retain)?;
        let key = claim.key;
        let owner = claim.activate(entry, charge);
        link.insert(key);
        Ok(Some(owner))
    }

    pub(in crate::buffer) fn bytes(&self) -> usize {
        // SAFETY: Registered lends this account while retaining its initialized charge.
        unsafe { (*self.resident.get()).assume_init_ref() }.capacity()
    }
}

// SAFETY: every published account edge retains this exact embedded count.
unsafe impl crate::mem::raw::Edge for Account<'_> {
    fn count(&self) -> &crate::mem::raw::Count {
        crate::mem::raw::Edge::count(&self.prefix.0)
    }
}

// SAFETY: live edges retain the initialized registration and its pool-slot root.
unsafe impl crate::mem::raw::Target for Account<'_> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: Last transfers this exact terminal account before any reuse.
        unsafe { Retired::new(pointer) }.finish();
    }
}

/// One registered pool edge carrying its resident-domain lifetime.
#[derive(Debug)]
#[repr(transparent)]
pub(in crate::buffer) struct Registered<'d> {
    owner: crate::mem::raw::Owner<Account<'d>>,
}

impl<'d> Registered<'d> {
    /// # Safety
    /// `ptr` transfers one live edge charged to the domain branded by `'d`.
    pub(in crate::buffer) unsafe fn new(ptr: ptr::NonNull<Account<'d>>) -> Self {
        Self {
            // SAFETY: the caller transfers one live edge from this exact account.
            owner: unsafe { crate::mem::raw::Owner::from_raw(ptr) },
        }
    }
}

// SAFETY: the concrete edge retains its original allocation, ownership, and lifetime.
unsafe impl<'d> buffer::raw::Retain for Registered<'d> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            owner: self.owner.try_clone()?,
        })
    }
}

impl<'d> backing::Owner<'d> for Registered<'d> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        Some(backing::Root::Accounted(self.owner.get()))
    }
}

const _: () = {
    assert!(size_of::<Prefix>() == size_of::<regions::Prefix>());
    assert!(size_of::<Accounts<'static>>() == 24);
    assert!(size_of::<Account<'static>>() == 32);
    assert!(size_of::<Active<'static, 'static>>() == size_of::<usize>());
    assert!(size_of::<Retired>() == size_of::<usize>());
    assert!(size_of::<Registered<'static>>() == size_of::<usize>());
    assert!(size_of::<Option<Registered<'static>>>() == size_of::<usize>());
};

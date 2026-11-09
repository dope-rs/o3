use std::{cell, mem, ptr};

use pool::state;

use crate::{
    buffer,
    buffer::{
        pool,
        pool::owners::{Free, Key},
        resident::{self, staging},
        storage::{backing, regions},
    },
    mem::rc,
};

#[repr(C, align(8))]
pub(in crate::buffer) struct Accounts<'d> {
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
        use crate::mem::raw::Edge;

        let account = self.account;
        let ptr = ptr::NonNull::from(account);
        // SAFETY: Claim proves this vacant prefix contains only its exact
        // Accounts free-list successor.
        unsafe { self.table.free.pop(account.prefix.word()) };
        // SAFETY: the claimed Account's resident cell is uninitialized and
        // exclusively owned by this unpublished transition.
        unsafe { *account.resident.get() = mem::MaybeUninit::new(charge) };
        let slot = mem::ManuallyDrop::new(slot);
        // SAFETY: Claim owns the vacant Account and its initialized charge;
        // the retained slot key is written before publishing the first edge.
        unsafe {
            account.prefix.write(slot.key.get());
            account.prefix.activate();
        }
        // SAFETY: activation created the first external account edge.
        unsafe { Registered::from_raw(ptr) }
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
        unsafe { self.0.prefix.try_retain() }.map_err(resident::AllocError::Retain)?;
        // SAFETY: the retained edge is branded by the checked budget domain.
        Ok(Some(unsafe {
            Registered::from_raw(ptr::NonNull::from(self.0))
        }))
    }
}

impl<'d> Accounts<'d> {
    pub(in crate::buffer) fn try_grow<I: buffer::raw::SliceExt>(
        reserve: staging::Reserve<
            (&mut pool::Span<super::Resident<'d>>, &mut u32),
            &super::Measured<I>,
        >,
    ) -> Result<bool, resident::AllocError> {
        let ((span, head), capacity, input) = reserve.into_parts();
        let entry = span.entry();
        let core = entry.core();
        let header = core.header();
        let previous = header.capacity();
        if capacity <= previous
            || header.slots() != 1
            || !header.is_detached()
            || !entry.is_unique()
        {
            return Ok(false);
        }
        let shape = super::Shape::<super::Accounted>::try_new(pool::NonEmpty::new(capacity).into())
            .map_err(resident::AllocError::Layout)?;
        // SAFETY: the detached one-slot block has one live edge and no
        // active account. Its root charge is exclusively owned here.
        let accounts = unsafe { Self::ptr(core).as_mut() };
        accounts
            .root
            .try_resize(capacity.get() as usize)
            .map_err(resident::AllocError::Budget)?;
        let block = match super::Block::<super::Accounted, state::Uninit>::allocate(shape) {
            Ok(block) => block,
            Err(error) => {
                accounts.root.truncate(previous.get() as usize);
                return Err(resident::AllocError::Exhausted(error));
            }
        };
        // SAFETY: transfer the sole root after all fallible work. The old block
        // will be deallocated without dropping this charge again.
        let root = unsafe { mem::ManuallyDrop::take(&mut accounts.root) };
        // SAFETY: the one-slot shape reserves one account and root covers its capacity.
        let next = unsafe { block.account(root, 1) };
        // SAFETY: the new block has a nonempty slot list with no published owner.
        let seed = unsafe { super::Seed::from_ptr(next) };
        // SAFETY: Slot retains this span's initialized suffix and admitted target.
        let bytes = unsafe { span.as_slice().get_unchecked(*head as usize..) };
        let next =
            pool::Span::from_seed(seed).copy_in((bytes, input.input()), bytes.len() + input.len());
        // SAFETY: the copy ended every payload borrow. The sole old edge and
        // its transferred root must not run Drop after deallocation.
        unsafe {
            core.deallocate();
            ptr::write(span, next);
        }
        *head = 0;
        Ok(true)
    }

    #[inline(always)]
    pub(super) unsafe fn initialize(
        base: super::Base<super::Accounted>,
        slots: u32,
        root: resident::Charge<'d>,
        limit: u32,
    ) -> super::Base<super::Resident<'d>> {
        // SAFETY: the accounted geometry is unchanged; this initialization
        // transfers the exact domain charge into its resident account table.
        let base = unsafe { super::Base::from_raw(base.as_raw().cast()) };
        // SAFETY: this resident shape reserves exactly these slots, initially unassociated.
        unsafe { super::Slot::initialize(base, slots, cell::Cell::new(Key::<true>::encode(None))) };
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
}

impl<'d, S> super::Ptr<super::Resident<'d>, S> {
    /// # Safety
    /// The consumed core is the terminal edge of this exact resident allocation.
    pub(in crate::buffer) unsafe fn uncharge(self) {
        // SAFETY: the accounted header owns one initialized Accounts table.
        let table = unsafe { Accounts::ptr(self).as_mut() };
        // SAFETY: terminal allocation ownership returns its retained charge once.
        unsafe { mem::ManuallyDrop::drop(&mut table.root) };
    }
}

#[repr(C, align(8))]
#[derive(Debug)]
pub(in crate::buffer) struct Account<'d> {
    prefix: regions::Prefix,
    core: super::Base<super::Resident<'d>>,
    resident: cell::UnsafeCell<mem::MaybeUninit<resident::Charge<'d>>>,
}

impl<'d> Account<'d> {
    const fn new(core: super::Base<super::Resident<'d>>, next: Option<Key<true>>) -> Self {
        Self {
            prefix: regions::Prefix::empty(Key::<true>::encode(next)),
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
        let link = &entry.slot().link;
        // SAFETY: the live slot's association is empty or its exact active account key.
        if let Some(key) = unsafe { Key::<true>::decode(link.get()) } {
            // SAFETY: this association was published after its account activated.
            let active = Active(unsafe { Accounts::entry(entry, key) });
            return active.try_retain(*budget);
        }
        let Some(claim) = Claim::new(entry) else {
            return Ok(None);
        };
        let charge = budget
            .try_charge(entry.capacity().get() as usize)
            .map_err(resident::AllocError::Budget)?;
        let entry = entry.try_retain().map_err(resident::AllocError::Retain)?;
        let key = claim.key;
        let owner = claim.activate(entry, charge);
        link.set(Key::encode(Some(key)));
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
        crate::mem::raw::Edge::count(&self.prefix)
    }
}

// SAFETY: live edges retain the initialized registration and its pool-slot root.
unsafe impl crate::mem::raw::Target for Account<'_> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        let (core, slot) = {
            // SAFETY: Last transfers this exact terminal account before any reuse.
            let account = unsafe { pointer.as_ref() };
            // SAFETY: the terminal account retains its exact linked slot.
            let slot = unsafe { Key::new(account.prefix.word()) };
            let core = account.core();
            let table = Accounts::ptr(core);
            // SAFETY: this terminal account is one published table cell.
            let first = unsafe { Accounts::cells(table.as_ptr()) };
            // SAFETY: this terminal account belongs to the same bounded u32 array.
            let index = unsafe { pointer.as_ptr().offset_from_unsigned(first) };
            let key = unsafe { Key::<true>::new(index as u32) };
            // SAFETY: terminal Account ownership keeps its accounted table live.
            let table = unsafe { table.as_ref() };
            // SAFETY: activation initialized the resident domain and terminal
            // ownership consumes it once before publishing this vacant account.
            unsafe { (*account.resident.get()).assume_init_drop() };
            // SAFETY: this terminal Account is the exact link published in `slot`.
            unsafe { core.ptr(slot).as_ref() }
                .link
                .set(Key::<true>::encode(None));
            // SAFETY: terminal ownership writes this account's vacant successor
            // before the common head publishes its exact key.
            unsafe {
                table
                    .free
                    .push(key, |next| account.prefix.write(Key::encode(next)))
            };
            (core, slot)
        };
        // SAFETY: all allocation borrows ended; transfer the slot edge retained at activation.
        unsafe { super::Active::release(core, slot) };
    }
}

/// One registered pool edge carrying its resident-domain lifetime.
pub(in crate::buffer) type Registered<'d> = crate::mem::raw::Owner<Account<'d>>;

// SAFETY: the concrete edge retains its original allocation, ownership, and lifetime.
unsafe impl<'d> buffer::raw::Retain for Registered<'d> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        self.try_clone()
    }
}

impl<'d> backing::Owner<'d> for Registered<'d> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        Some(backing::Root::Accounted(self.get()))
    }
}

const _: () = {
    assert!(size_of::<Accounts<'static>>() == 24);
    assert!(size_of::<Account<'static>>() == 32);
    assert!(size_of::<Active<'static, 'static>>() == size_of::<usize>());
    assert!(size_of::<Registered<'static>>() == size_of::<usize>());
    assert!(size_of::<Option<Registered<'static>>>() == size_of::<usize>());
};

use std::{
    cell, marker,
    mem::{MaybeUninit, offset_of, size_of},
    ops, ptr,
};

use provided::account;

use crate::{
    buffer::{
        resident,
        storage::{provided, provided::arena},
    },
    mem::raw::{self, Edge},
};

/// Only admitted registrations can change this arena's outstanding count.
#[repr(transparent)]
#[derive(Default)]
pub(super) struct Counter(cell::Cell<u32>);

/// The embedded account domain; a live count retains its initialized debit.
#[derive(Debug)]
#[repr(C)]
pub(in crate::buffer::storage::provided) struct Registration<'d> {
    count: raw::Count,
    pub(super) value: cell::UnsafeCell<MaybeUninit<resident::Charge<'d>>>,
}

/// The hidden scope's authority to reclaim an initialized item.
#[derive(Clone, Copy)]
enum State {
    Empty,
    Ready,
    External,
}

/// Scope-owned metadata; the free and returned lists share one phase-owned link.
#[repr(C, align(8))]
pub(in crate::buffer::storage::provided) struct Entry<'d> {
    state: cell::Cell<State>,
    pub(super) resident: cell::Cell<MaybeUninit<u32>>,
    pub(super) domain: &'d arena::State<'d>,
    pub(super) next: arena::nodes::Link<'d>,
    registration: Registration<'d>,
}

impl<'d> Entry<'d> {
    pub(in crate::buffer::storage::provided) const fn new(
        domain: &'d arena::State<'d>,
        resident: MaybeUninit<u32>,
    ) -> Self {
        Self {
            state: cell::Cell::new(State::Empty),
            resident: cell::Cell::new(resident),
            domain,
            next: cell::Cell::new(None),
            registration: Registration {
                count: raw::Count::empty(),
                value: cell::UnsafeCell::new(MaybeUninit::uninit()),
            },
        }
    }

    pub(super) fn initialize(node: arena::Empty<'d>) -> arena::Ready<'d> {
        // SAFETY: the vacancy owns the slot whose item was just initialized.
        unsafe { node.entry.as_ref() }.state.set(State::Ready);
        arena::Ready { entry: node.entry }
    }

    pub(super) fn activate<T, L>(ready: arena::Returned<'d, T, L>) -> provided::Owner<'d> {
        let entry = ready.node.entry;
        // SAFETY: publication consumes the sole detached initialized item.
        unsafe { entry.as_ref() }.state.set(State::External);
        provided::Owner { entry }
    }

    /// # Safety
    /// The pointer retains the complete slot allocation for its active scope.
    /// Its resident was initialized by the fixed scope or exact admission.
    pub(in crate::buffer::storage::provided) unsafe fn resident(
        pointer: ptr::NonNull<Self>,
    ) -> u32 {
        // SAFETY: the caller retains the slot and its initialized resident value.
        unsafe { pointer.as_ref().resident.get().assume_init() }
    }

    pub(in crate::buffer::storage::provided) fn try_account(
        owner: &provided::Owner<'d>,
        budget: resident::Budget<'d>,
    ) -> Result<provided::Account<'d>, account::Cause> {
        let pointer = owner.entry;
        // SAFETY: the owner retains this scope's stable slot. Occupied
        // registrations remain shared; admission invokes no user callbacks.
        let entry: &'d Self = unsafe { pointer.as_ref() };
        let registration = &entry.registration;
        if !registration.is_empty() {
            // SAFETY: the published count retains the initialized resident charge.
            let charge = unsafe { (*registration.value.get()).assume_init_ref() };
            if !ptr::eq(charge.budget().core(), budget.core()) {
                return Err(account::Cause::Foreign);
            }
            // SAFETY: the slot owner retains this proved live registration;
            // no callbacks or edge releases intervene before the increment.
            unsafe { registration.try_retain() }.map_err(account::Cause::Retain)?;
        } else {
            let domain = entry.domain;
            let count = domain.accounted.0.get();
            let limit = domain.limit;
            if count >= limit {
                return Err(account::Cause::Exhausted { count, limit });
            }
            // SAFETY: the owner retains the slot's original resident layout.
            let charge = budget
                .try_charge(unsafe { Self::resident(pointer) } as usize)
                .map_err(account::Cause::Budget)?;
            domain.accounted.0.set(count + 1);
            // SAFETY: this empty registration has no issued account. Initialize
            // its sole debit before publishing the first edge; no callbacks run.
            unsafe {
                (*registration.value.get()).write(charge);
                registration.activate();
            }
        }
        // SAFETY: admission transferred one live embedded edge. Deriving its
        // address from the complete slot preserves provenance for finalization.
        Ok(provided::Account {
            registration: unsafe {
                raw::Owner::from_raw(pointer.byte_add(offset_of!(Self, registration)).cast())
            },
        })
    }

    /// # Safety
    /// The registration is initialized and all its issued access has ended.
    /// Consume it once before returning the slot or initializing another charge.
    unsafe fn unaccount(&self) -> &'d arena::State<'d> {
        let domain = self.domain;
        domain.accounted.0.set(domain.accounted.0.get() - 1);
        // SAFETY: terminal registration ownership consumes its sole resident debit.
        unsafe { (*self.registration.value.get()).assume_init_drop() };
        domain
    }

    /// # Safety
    /// The owner must retain the sole live external edge, consumed exactly once here.
    pub(super) unsafe fn release(owner: &mut provided::Owner<'d>) {
        // SAFETY: the owner retains the sole external edge to the live slot.
        let entry = unsafe { owner.entry.as_ref() };
        entry.state.set(State::Ready);
        if entry.registration.is_empty() {
            entry.domain.finish(arena::Ready { entry: owner.entry });
        }
    }

    pub(super) fn recycle(node: arena::Ready<'d>) {
        // SAFETY: the consumed ready node retains the exact slot through its free-list return.
        let entry = unsafe { node.entry.as_ref() };
        entry.state.set(State::Empty);
        entry.domain.push_free(arena::Empty { entry: node.entry });
    }

    /// # Safety
    /// The hidden scope has ended every issued access to the complete slot.
    pub(in crate::buffer::storage::provided) unsafe fn revoke<T, L>(
        slot: &'d arena::Slot<'d, T, L>,
    ) -> ops::ControlFlow<(), arena::Returned<'d, T, L>> {
        let pointer = ptr::NonNull::from(slot).cast::<Self>();
        let entry = &slot.entry;
        match entry.state.get() {
            State::Empty => ops::ControlFlow::Break(()),
            State::Ready | State::External => {
                if !entry.registration.is_empty() {
                    // SAFETY: the ended scope owns even forgotten account edges.
                    // No issued access or count operation survives reclamation;
                    // the next generation initializes this slot before reuse.
                    unsafe { entry.unaccount() };
                }
                ops::ControlFlow::Continue(arena::Returned {
                    node: arena::Ready { entry: pointer },
                    domain: marker::PhantomData,
                })
            }
        }
    }
}

// SAFETY: every account edge retains this exact slot and embedded count.
unsafe impl raw::Edge for Registration<'_> {
    fn count(&self) -> &raw::Count {
        &self.count
    }
}

// SAFETY: account edges retain the initialized debit; its finalizer owns both
// refunds before returning the slot, without callbacks or premature reuse.
unsafe impl<'d> raw::Target for Registration<'d> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: activation retained the complete slot's provenance for this
        // exact embedded registration in the stable repr(C) entry.
        let pointer =
            unsafe { pointer.byte_sub(offset_of!(Entry<'d>, registration)) }.cast::<Entry<'d>>();
        // SAFETY: the last counted edge ended and set this registration's count to zero.
        let entry = unsafe { pointer.as_ref() };
        let domain = unsafe { entry.unaccount() };
        if matches!(entry.state.get(), State::Ready) {
            domain.finish(arena::Ready { entry: pointer });
        }
    }
}

const _: () = {
    assert!(size_of::<Registration<'_>>() == 3 * size_of::<usize>());
    assert!(size_of::<Entry<'_>>() == 6 * size_of::<usize>());
    assert!(align_of::<Entry<'_>>() == 8);
};

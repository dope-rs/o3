use std::{
    cell, marker,
    mem::{ManuallyDrop, size_of},
    ops, ptr,
};

use provided::account;

use crate::buffer::{
    resident,
    storage::{provided, provided::arena},
};

/// The hidden scope's authority to reclaim an initialized item.
#[derive(Clone, Copy)]
enum State {
    Empty,
    Ready,
    External,
}

/// Scope-owned metadata with permanently initialized, separately typed links.
#[repr(C, align(8))]
pub(in crate::buffer::storage::provided) struct Entry<'d> {
    state: cell::Cell<State>,
    pub(super) domain: &'d arena::State<'d>,
    pub(super) free: cell::Cell<Option<arena::Empty<'d>>>,
    pub(super) returned: cell::Cell<Option<arena::Ready<'d>>>,
    pub(super) charge: cell::UnsafeCell<Option<ManuallyDrop<arena::Charge<'d>>>>,
}

impl<'d> Entry<'d> {
    pub(in crate::buffer::storage::provided) const fn new(domain: &'d arena::State<'d>) -> Self {
        Self {
            state: cell::Cell::new(State::Empty),
            domain,
            free: cell::Cell::new(None),
            returned: cell::Cell::new(None),
            charge: cell::UnsafeCell::new(None),
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
    /// The complete newly initialized slot belongs to this scope for `'d`.
    pub(in crate::buffer::storage::provided) unsafe fn seed<T, L>(slot: &'d arena::Slot<'d, T, L>) {
        let entry = ptr::NonNull::from(slot).cast::<Self>();
        slot.entry.domain.push_free(arena::Empty { entry });
    }

    /// # Safety
    /// The pointer retains the complete slot allocation for its active scope.
    pub(in crate::buffer::storage::provided) unsafe fn resident(
        pointer: ptr::NonNull<Self>,
    ) -> u32 {
        // SAFETY: the caller retains the complete live slot, including its leading Entry.
        match unsafe { pointer.as_ref() }.domain.mode() {
            provided::Mode::Fixed(resident) => resident.get(),
            provided::Mode::Exact => {
                // SAFETY: the exact Slot places its resident cell after Entry.
                let value = unsafe {
                    &*pointer
                        .as_ptr()
                        .byte_add(size_of::<Self>())
                        .cast::<cell::Cell<u32>>()
                };
                value.get()
            }
        }
    }

    pub(in crate::buffer::storage::provided) fn try_account(
        owner: &provided::Owner<'d>,
        budget: resident::Budget<'d>,
    ) -> Result<provided::Account<'d>, account::Cause> {
        let pointer = owner.entry;
        // SAFETY: the owner retains this scope's stable slot. Occupied
        // registrations remain shared; admission invokes no user callbacks.
        let entry: &'d Self = unsafe { pointer.as_ref() };
        let charge = unsafe { &*entry.charge.get() };
        if let Some(charge) = charge {
            if !ptr::eq(charge.budget().core(), budget.core()) {
                return Err(account::Cause::Foreign);
            }
            charge.try_retain().map_err(account::Cause::Retain)?;
            return Ok(provided::Account::new(charge));
        }
        // SAFETY: the vacant registration has no Account or escaping borrow.
        let vacant = unsafe { &mut *entry.charge.get() };
        let charge = vacant.insert(ManuallyDrop::new(arena::Charge::try_new(owner, budget)?));
        Ok(provided::Account::new(charge))
    }

    pub(super) fn unregister(charge: arena::Charge<'d>) {
        // SAFETY: the last Account moved this exact initialized registration;
        // overwriting its former storage retires it without dropping it twice.
        let entry = unsafe { charge.entry().as_ref() };
        unsafe { entry.charge.get().write(None) };
        let pointer = charge.entry();
        drop(charge);
        if matches!(entry.state.get(), State::Ready) {
            entry.domain.finish(arena::Ready { entry: pointer });
        }
    }

    /// # Safety
    /// The owner must retain the sole live external edge, consumed exactly once here.
    pub(super) unsafe fn release(owner: &mut provided::Owner<'d>) {
        // SAFETY: the owner retains the sole external edge to the live slot.
        let entry = unsafe { owner.entry.as_ref() };
        entry.state.set(State::Ready);
        // SAFETY: release consumes the external edge before testing the remaining account.
        if unsafe { &*entry.charge.get() }.is_none() {
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
                // SAFETY: terminal scope authority now owns every registration,
                // including accounts forgotten inside the ended callback.
                if let Some(charge) = unsafe { entry.charge.get().replace(None) } {
                    drop(ManuallyDrop::into_inner(charge));
                }
                ops::ControlFlow::Continue(arena::Returned {
                    node: arena::Ready { entry: pointer },
                    domain: marker::PhantomData,
                })
            }
        }
    }
}

const _: () = {
    assert!(size_of::<Entry<'_>>() == usize::BITS as usize);
    assert!(align_of::<Entry<'_>>() == 8);
};

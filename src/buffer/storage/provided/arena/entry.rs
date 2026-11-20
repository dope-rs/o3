use std::{
    cell, marker,
    mem::{MaybeUninit, size_of},
    num::NonZero,
    ops, ptr,
};

use provided::account;

use crate::{
    buffer::{
        raw::{Policy as _, Projection},
        resident, storage,
        storage::{provided, provided::arena},
    },
    mem::raw::{self, Edge},
    mem::slot,
};

/// Only admitted registrations can change this arena's outstanding count.
#[repr(transparent)]
#[derive(Default)]
pub(super) struct Counter(cell::Cell<u32>);

/// A live edge retains the slot and its paying budget.
#[derive(Debug)]
#[repr(transparent)]
pub(super) struct Registration<'d> {
    count: raw::Count,
    scope: marker::PhantomData<fn(&'d ()) -> &'d ()>,
}

/// The hidden scope's authority to reclaim an initialized item.
#[derive(Clone, Copy)]
enum State {
    Empty,
    ReturnOnLastAccount,
    Occupied,
}

/// An account can only exist while the slot is detached from both lists.
#[derive(Clone, Copy)]
pub(super) union Data<'d> {
    pub(super) free: Option<ptr::NonNull<Entry<'d>>>,
    pub(super) returned: ptr::NonNull<Entry<'d>>,
    budget: resident::Budget<'d>,
}

#[repr(C, align(8))]
pub(in crate::buffer::storage::provided) struct Entry<'d> {
    registration: Registration<'d>,
    state: cell::Cell<State>,
    pub(super) resident: cell::Cell<MaybeUninit<usize>>,
    pub(super) data: cell::Cell<Data<'d>>,
    pub(super) domain: &'d arena::State<'d>,
}

impl<'d> storage::NonEmpty<provided::Prefix<'_, 'd, &[u8]>> {
    #[inline]
    pub(in crate::buffer::storage::provided) fn try_account<A: resident::Admission>(
        self,
        budget: resident::Budget<'d>,
        admission: A,
    ) -> Result<
        ops::ControlFlow<A::Declined, storage::Shared<Option<provided::Account<'d>>>>,
        account::Cause,
    > {
        let prefix = self.into_storage();
        let pointer = prefix.owner.entry;
        // SAFETY: the owner retains this scope's stable slot. Occupied
        // registrations remain shared; admission invokes no user callbacks.
        let entry: &'d Entry<'d> = unsafe { pointer.as_ref() };
        let registration = &entry.registration;
        if !registration.is_empty() {
            // SAFETY: the live account keeps this slot detached with its original paying budget.
            let paying = unsafe { entry.data.get().budget };
            if !ptr::eq(paying.core(), budget.core()) {
                return Err(account::Cause::Foreign);
            }
            if let ops::ControlFlow::Break(declined) = admission.check(budget, 0) {
                return Ok(ops::ControlFlow::Break(declined));
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
            // SAFETY: the admitted projection fits this slot's initialized ceiling,
            // and the nonempty prefix retains at least one byte of that projection.
            let resident = unsafe { NonZero::new_unchecked(*entry.capacity() as u32) };
            if let ops::ControlFlow::Break(declined) =
                admission.check(budget, resident.get() as usize)
            {
                return Ok(ops::ControlFlow::Break(declined));
            }
            budget
                .try_acquire(resident)
                .map_err(account::Cause::Budget)?;
            domain.accounted.0.set(count + 1);
            entry.data.set(Data { budget });
            // SAFETY: the detached slot owns the debit and its count is still empty.
            unsafe { registration.activate() };
        }
        // SAFETY: admission transfers one edge to the first field of this repr(C) entry.
        let owner = provided::Account {
            registration: unsafe { raw::Owner::from_raw(pointer.cast()) },
        };
        // SAFETY: the counted edge retains the same slot as these admitted prefix bytes.
        Ok(ops::ControlFlow::Continue(unsafe {
            storage::Shared::assume(prefix.bytes, Some(owner))
        }))
    }
}

impl<'d> Entry<'d> {
    pub(super) fn new(
        domain: &'d arena::State<'d>,
        resident: MaybeUninit<usize>,
        next: Option<arena::Empty<'d>>,
    ) -> Self {
        Self {
            registration: Registration {
                count: raw::Count::empty(),
                scope: marker::PhantomData,
            },
            state: cell::Cell::new(State::Empty),
            resident: cell::Cell::new(resident),
            data: cell::Cell::new(Data {
                free: next.map(|node| node.entry),
            }),
            domain,
        }
    }

    /// # Safety
    /// Admission initialized this u32 ceiling and the slot cannot be reused during the borrow.
    pub(in crate::buffer::storage::provided) unsafe fn capacity(&self) -> &usize {
        unsafe { (&*self.resident.as_ptr()).assume_init_ref() }
    }

    /// # Safety
    /// The registration is initialized and all its issued access has ended.
    /// Consume it once before returning the slot or initializing another charge.
    unsafe fn refund(&self) {
        // SAFETY: the registration still holds its budget and the same ceiling it debited.
        unsafe { self.data.get().budget.release(*self.capacity()) };
    }

    /// # Safety
    /// The owner must retain the sole live external edge, consumed exactly once here.
    pub(super) unsafe fn release(owner: &mut provided::Owner<'d>) {
        // SAFETY: the owner retains the sole external edge to the live slot.
        let entry = unsafe { owner.entry.as_ref() };
        if entry.registration.is_empty() {
            entry.domain.finish(arena::Ready { entry: owner.entry });
        } else {
            entry.state.set(State::ReturnOnLastAccount);
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
    pub(super) unsafe fn revoke<T, L>(
        slot: &'d arena::Slot<'d, T, L>,
    ) -> Option<slot::Occupied<'d, T>> {
        let entry = &slot.entry;
        match entry.state.get() {
            State::Empty => None,
            State::ReturnOnLastAccount | State::Occupied => {
                if !entry.registration.is_empty() {
                    // SAFETY: the ended scope owns even forgotten account edges.
                    // No issued access or count operation survives reclamation;
                    // the next generation initializes this slot before reuse.
                    unsafe { entry.refund() };
                }
                // SAFETY: this ended generation owns the initialized item until its release.
                Some(unsafe { slot.occupied() })
            }
        }
    }
}

impl<'d> arena::Ready<'d> {
    pub(super) fn publish(self, view: Projection<'d, '_>) -> provided::Unique<'d> {
        // SAFETY: publication consumes the detached initialized slot for this projection.
        unsafe { self.entry.as_ref() }.state.set(State::Occupied);
        provided::Unique::new(view, provided::Owner { entry: self.entry })
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
        // Activation issued the first field's address from the complete repr(C) slot.
        let pointer = pointer.cast::<Entry<'d>>();
        // SAFETY: the last counted edge ended and set this registration's count to zero.
        let entry = unsafe { pointer.as_ref() };
        let domain = entry.domain;
        domain.accounted.0.set(domain.accounted.0.get() - 1);
        // SAFETY: this last live edge owns the registration's remaining debit.
        unsafe { entry.refund() };
        if matches!(entry.state.get(), State::ReturnOnLastAccount) {
            domain.finish(arena::Ready { entry: pointer });
        }
    }
}

const _: () = {
    assert!(size_of::<Entry<'_>>() == 4 * size_of::<usize>());
    assert!(align_of::<Entry<'_>>() == 8);
};

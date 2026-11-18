use crate::{buffer, buffer::storage::provided::arena};

pub(in crate::buffer::storage::provided) mod account;
pub(in crate::buffer::storage::provided) mod owner;
pub(in crate::buffer::storage::provided) mod root;
pub(in crate::buffer::storage::provided) mod vacant;

mod domain;
mod entry;
mod nodes;

use std::{cell, marker, mem, num, pin, ptr, slice};

pub(in crate::buffer::storage::provided) use domain::{Domain, State};
pub(in crate::buffer::storage::provided) use entry::{Entry, Registration};
pub(in crate::buffer::storage::provided) use nodes::{Empty, Ready};
use root::Root;

use crate::{
    alloc,
    alloc::tail,
    base::cell::{brand, region},
    buffer::storage::provided::fixed,
    mem::slot,
};

/// Inactive allocation; no item or charge exists outside its private scope.
#[repr(transparent)]
pub struct Arena<T, C = (), L = cell::Cell<u32>> {
    entries: tail::Block<Header<C>, mem::MaybeUninit<Slot<'static, T, L>>, num::NonZero<u32>>,
}

#[repr(C)]
struct Header<C> {
    memory: cell::UnsafeCell<mem::MaybeUninit<State<'static>>>,
    context: C,
}

#[repr(C)]
struct Active<'d, T, C, L> {
    domain: arena::Domain<'d, T, L>,
    context: C,
}

pub(in crate::buffer::storage::provided) struct Scope<'d, T, L> {
    slots: slice::Iter<'d, arena::Slot<'d, T, L>>,
}

impl<T, C, L> Arena<T, C, L> {
    /// Allocates the exact arena before its context is constructed.
    pub fn try_allocate(
        entries: num::NonZero<u32>,
    ) -> Result<impl FnOnce(C) -> Self, alloc::Error> {
        let memory = tail::Plan::try_new(entries)?.try_allocate()?;
        Ok(move |context| Self::initialize(memory, context))
    }

    fn initialize(
        memory: tail::Uninit<Header<C>, Slot<'static, T, L>, num::NonZero<u32>>,
        context: C,
    ) -> Self {
        let header = Header {
            memory: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
            context,
        };
        Self {
            entries: memory.init(header),
        }
    }

    pub fn context(&self) -> &C {
        &tail::View::new(&self.entries).header().context
    }
    pub fn capacity(&self) -> num::NonZero<u32> {
        self.entries.extent()
    }
    pub fn into_context(self) -> C {
        self.entries.into_header().context
    }

    /// # Safety
    /// The arena is inactive with every prior scope reclaimed. End all issued access,
    /// then finish the returned scope before releasing its exclusive generation borrow.
    /// For fixed storage, `resident` is initialized to this context's admitted ceiling.
    pub(in crate::buffer::storage::provided) unsafe fn enter<'d>(
        &'d mut self,
        limit: u32,
        resident: mem::MaybeUninit<u32>,
    ) -> (Root<'d, T, C, L>, Scope<'d, T, L>) {
        const {
            assert!(size_of::<Header<C>>() == size_of::<Active<'_, T, C, L>>());
            assert!(align_of::<Header<C>>() == align_of::<Active<'_, T, C, L>>());
            assert!(
                mem::offset_of!(Header<C>, context)
                    == mem::offset_of!(Active<'_, T, C, L>, context)
            );
        }
        let capacity = self.entries.extent();
        let (header, values) = self.entries.parts_mut();
        // SAFETY: this inactive memory is uninitialized and exclusively owned;
        // initialization preserves the original header and tail allocation.
        unsafe {
            header
                .memory
                .get()
                .cast::<arena::Domain<'d, T, L>>()
                .write(arena::Domain::new(capacity, limit))
        };
        // SAFETY: the assertions preserve the live context's layout, and the
        // domain was initialized in the disjoint header storage immediately above.
        let header = unsafe { &*ptr::from_ref(header).cast::<Active<'d, T, C, L>>() };
        let mut free = None;
        for slot in values.iter_mut() {
            let entry = ptr::NonNull::from(slot).cast::<arena::Entry<'d>>();
            // SAFETY: this generation exclusively initializes each complete slot;
            // its item remains in UnsafeCell<MaybeUninit<T>> until insertion.
            unsafe {
                entry
                    .as_ptr()
                    .write(arena::Entry::new(&header.domain.state, resident, free))
            };
            free = Some(arena::Empty { entry });
        }
        header.domain.state.free.set(free);
        // SAFETY: every Entry is initialized; the remaining Slot fields are
        // PhantomData and UnsafeCell<MaybeUninit<T>>, which admit uninitialized bytes.
        let slots = unsafe {
            slice::from_raw_parts(
                values.as_ptr().cast::<arena::Slot<'d, T, L>>(),
                values.len(),
            )
        };
        (
            Root { header },
            Scope {
                slots: slots.iter(),
            },
        )
    }

    fn scope_in<S, R>(
        &mut self,
        state: pin::Pin<&mut S>,
        resident: mem::MaybeUninit<u32>,
        run: impl for<'d> FnOnce(
            region::Token<region::Identity<'d>>,
            Root<'d, T, C, L>,
            pin::Pin<&'d mut S>,
        ) -> R,
    ) -> R {
        let mut input = (self, state);
        region::Token::scope_pinned(pin::Pin::new(&mut input), |token, input| {
            let (arena, state) = input.get_mut();
            // SAFETY: the complete generative call borrows this inactive allocation;
            // the hidden scope reclaims every issued capability before it returns.
            let (root, scope) = unsafe { arena.enter(arena.capacity().get(), resident) };
            let result = run(token, root, state.as_mut());
            scope.finish(drop);
            result
        })
    }
}

impl<T, C: buffer::raw::Fixed<T>> Arena<T, C, ()> {
    /// Limits distinct accounted slots to `accounts`, capped by this arena's capacity.
    pub fn run<S, O: fixed::Operation<S, T, C>>(
        &mut self,
        state: pin::Pin<&mut S>,
        accounts: u32,
        terminal: &mut O::Terminal,
        op: O,
    ) -> O::Output {
        let resident = mem::MaybeUninit::new(self.context().resident().get());
        let mut input = (self, state, terminal);
        brand::scope_pinned(pin::Pin::new(&mut input), |token, regions, input| {
            let (arena, state, terminal) = input.get_mut();
            // SAFETY: this sole generation borrow retains the allocation until
            // the operation ends and the hidden scope reclaims every slot.
            let (arena, scope) = unsafe { arena.enter(accounts, resident) };
            let terminal = fixed::Loan {
                target: &mut **terminal,
                brand: regions.0.proof(),
            };
            let (output, mut end) = op.run(token, regions, arena, state.as_mut(), terminal);
            scope.finish(|item| end.release(item.into_inner()));
            output
        })
    }
}

impl<'d, T, L> Scope<'d, T, L> {
    fn finish(self, mut release: impl FnMut(slot::Occupied<'d, T>)) {
        for slot in self.slots {
            // SAFETY: the callback ended all issued access; budgets and the
            // operation's final authority remain alive throughout reclamation.
            if let Some(item) = unsafe { arena::Entry::revoke(slot) } {
                release(item);
            }
        }
    }
}

const _: () = {
    assert!(!mem::needs_drop::<arena::Domain<'_, (), ()>>());
    assert!(size_of::<Arena<(), (), ()>>() == size_of::<usize>());
    assert!(size_of::<Root<'_, (), (), ()>>() == size_of::<usize>());
};

impl<T, C> Arena<T, C> {
    /// # Safety
    /// The raw Arenas contract retains the root and runs its terminal exactly once.
    pub(in crate::buffer) unsafe fn enter_scope(&mut self) -> (Root<'_, T, C>, impl FnOnce() + '_) {
        // SAFETY: the caller retains this inactive arena and executes the
        // returned terminal after all issued access ends, before releasing it.
        let (root, scope) =
            unsafe { self.enter(self.capacity().get(), mem::MaybeUninit::uninit()) };
        (root, move || scope.finish(drop))
    }

    pub fn scope<S, R>(
        &mut self,
        state: pin::Pin<&mut S>,
        run: impl for<'d> FnOnce(
            region::Token<region::Identity<'d>>,
            Root<'d, T, C>,
            pin::Pin<&'d mut S>,
        ) -> R,
    ) -> R {
        self.scope_in(state, mem::MaybeUninit::uninit(), run)
    }
}
impl<T, C: buffer::raw::Fixed<T>> Arena<T, C, ()> {
    pub fn scope<S, R>(
        &mut self,
        state: pin::Pin<&mut S>,
        run: impl for<'d> FnOnce(
            region::Token<region::Identity<'d>>,
            Root<'d, T, C, ()>,
            pin::Pin<&'d mut S>,
        ) -> R,
    ) -> R {
        let resident = mem::MaybeUninit::new(self.context().resident().get());
        self.scope_in(state, resident, run)
    }
}

#[repr(C, align(8))]
pub(in crate::buffer::storage::provided) struct Slot<'d, T, L> {
    entry: arena::Entry<'d>,
    layout: marker::PhantomData<L>,
    item: cell::UnsafeCell<mem::MaybeUninit<T>>,
}

impl<T> Slot<'_, T, cell::Cell<u32>> {
    pub(in crate::buffer::storage::provided) fn set_resident(&self, resident: u32) {
        self.entry.resident.set(mem::MaybeUninit::new(resident));
    }
}

impl<T, L> Slot<'_, T, L> {
    /// # Safety
    /// The caller exclusively owns the initialized item after all projections end.
    unsafe fn occupied(&self) -> slot::Occupied<'_, T> {
        // SAFETY: the slot retains this item at its final address for the exclusive loan.
        unsafe { slot::Vacant::new(&mut *self.item.get()).assume_init() }
    }
}

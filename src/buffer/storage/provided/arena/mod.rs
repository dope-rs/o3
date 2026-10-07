use charge::Charge;

use crate::{buffer, buffer::storage::provided::arena};

pub(in crate::buffer::storage::provided) mod account;
pub(in crate::buffer::storage::provided) mod owner;
pub(in crate::buffer::storage::provided) mod root;
pub(in crate::buffer::storage::provided) mod vacant;

pub(in crate::buffer::storage::provided) mod charge;
mod domain;
mod entry;
mod nodes;

use std::{cell, marker, mem, num, ops, pin, ptr, slice};

pub(in crate::buffer::storage::provided) use domain::{Domain, State};
pub(in crate::buffer::storage::provided) use entry::Entry;
pub(in crate::buffer::storage::provided) use nodes::{Empty, Ready};
use root::Root;

use crate::{
    alloc,
    alloc::tail,
    base::cell::{brand, region},
    buffer::storage::{
        provided,
        provided::{fixed, view},
    },
};

/// Inactive allocation; no item or charge exists outside its private scope.
#[repr(transparent)]
pub struct Arena<T, C = (), L = cell::Cell<u32>> {
    entries: tail::Block<Header<C>, Space<T, L>, num::NonZero<u32>>,
}

#[repr(C)]
struct Header<C> {
    memory: cell::UnsafeCell<[mem::MaybeUninit<usize>; 5]>,
    context: C,
}

#[repr(C, align(8))]
struct Space<T, L> {
    entry: cell::UnsafeCell<[mem::MaybeUninit<usize>; 8]>,
    resident: L,
    item: cell::UnsafeCell<mem::MaybeUninit<T>>,
}

#[repr(C)]
struct Active<'d, T, C, L> {
    domain: arena::Domain<'d, T, L>,
    context: C,
}

pub(in crate::buffer::storage::provided) struct Scope<'d, T, L> {
    slots: slice::Iter<'d, arena::Slot<'d, T, L>>,
}

impl<T, C, L: Default> Arena<T, C, L> {
    fn initialize(
        memory: tail::Uninit<Header<C>, Space<T, L>, num::NonZero<u32>>,
        context: C,
    ) -> Self {
        let header = Header {
            memory: cell::UnsafeCell::new([mem::MaybeUninit::uninit(); 5]),
            context,
        };
        Self {
            entries: memory.repeat_with(header, || Space {
                entry: cell::UnsafeCell::new([mem::MaybeUninit::uninit(); 8]),
                resident: L::default(),
                item: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
            }),
        }
    }
}

impl<T, C, L> Arena<T, C, L> {
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
    pub(in crate::buffer::storage::provided) unsafe fn enter<'d>(
        &'d mut self,
        limit: u32,
        mode: L::Setting,
    ) -> (Root<'d, T, C, L>, Scope<'d, T, L>)
    where
        L: arena::Layout,
    {
        const {
            assert!(size_of::<Header<C>>() == size_of::<Active<'_, T, C, L>>());
            assert!(align_of::<Header<C>>() == align_of::<Active<'_, T, C, L>>());
            assert!(
                mem::offset_of!(Header<C>, context)
                    == mem::offset_of!(Active<'_, T, C, L>, context)
            );
            assert!(size_of::<Space<T, L>>() == size_of::<arena::Slot<'_, T, L>>());
            assert!(align_of::<Space<T, L>>() == align_of::<arena::Slot<'_, T, L>>());
            assert!(
                mem::offset_of!(Space<T, L>, item) == mem::offset_of!(arena::Slot<'_, T, L>, item)
            );
        }
        let capacity = self.entries.extent();
        let header = tail::View::new(&self.entries).header();
        // SAFETY: this inactive memory is uninitialized and exclusively owned;
        // initialization preserves the original header and tail allocation.
        unsafe {
            header
                .memory
                .get()
                .cast::<arena::Domain<'d, T, L>>()
                .write(arena::Domain::new(capacity, limit, mode))
        };
        // SAFETY: the assertions preserve the live context's layout, and the
        // domain was initialized in the disjoint header storage immediately above.
        let header = unsafe { &*ptr::from_ref(header).cast::<Active<'d, T, C, L>>() };
        let values = tail::View::new(&self.entries).as_slice();
        for slot in values {
            // SAFETY: the inactive scope owns each entry's uninitialized storage;
            // its exact layout is asserted above and the active domain stays live.
            unsafe {
                slot.entry
                    .get()
                    .cast::<arena::Entry<'d>>()
                    .write(arena::Entry::new(&header.domain.state))
            };
        }
        // SAFETY: the exact validated tail now contains initialized typed entries.
        let slots = unsafe {
            slice::from_raw_parts(
                values.as_ptr().cast::<arena::Slot<'d, T, L>>(),
                values.len(),
            )
        };
        for slot in slots {
            // SAFETY: every complete slot is initialized, unpublished, and
            // retained by this exclusive arena scope for the whole generation.
            unsafe { arena::Entry::seed(slot) };
        }
        (
            Root { header },
            Scope {
                slots: slots.iter(),
            },
        )
    }
}

impl<T, C, L> Arena<T, C, L> {
    fn scope_in<S, R>(
        &mut self,
        state: pin::Pin<&mut S>,
        mode: L::Setting,
        run: impl for<'d> FnOnce(
            region::Token<region::Identity<'d>>,
            Root<'d, T, C, L>,
            pin::Pin<&'d mut S>,
        ) -> R,
    ) -> R
    where
        L: arena::Layout,
    {
        let mut input = (self, state);
        region::Token::scope_pinned(pin::Pin::new(&mut input), |token, input| {
            let (arena, state) = input.get_mut();
            // SAFETY: the complete generative call borrows this inactive allocation;
            // the hidden scope reclaims every issued capability before it returns.
            let (root, scope) = unsafe { arena.enter(arena.capacity().get(), mode) };
            let result = run(token, root, state.as_mut());
            scope.finish(arena::Returned::discard);
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
        let resident = self.context().resident();
        let mut input = (self, state, terminal);
        brand::Token::scope_pinned(pin::Pin::new(&mut input), |token, regions, input| {
            let (arena, state, terminal) = input.get_mut();
            // SAFETY: this sole generation borrow retains the allocation until
            // the operation ends and the hidden scope reclaims every slot.
            let (arena, scope) = unsafe { arena.enter(accounts, resident) };
            let terminal = fixed::Loan {
                target: &mut **terminal,
                brand: regions.0.proof(),
            };
            let (output, mut end) = op.run(token, regions, arena, state.as_mut(), terminal);
            scope.finish(|slot| end.release(slot.take()));
            output
        })
    }
}

impl<'d, T, L> Scope<'d, T, L> {
    pub(in crate::buffer::storage::provided) fn finish(
        self,
        mut release: impl FnMut(arena::Returned<'d, T, L>),
    ) {
        for slot in self.slots {
            // SAFETY: the callback ended all issued access; budgets and the
            // operation's final authority remain alive throughout reclamation.
            match unsafe { arena::Entry::revoke(slot) } {
                ops::ControlFlow::Break(()) => {}
                ops::ControlFlow::Continue(slot) => release(slot),
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
        let (root, scope) = unsafe { self.enter(self.capacity().get(), ()) };
        (root, move || scope.finish(Returned::discard))
    }

    pub fn try_with<E>(
        entries: num::NonZero<u32>,
        context: impl FnOnce() -> Result<C, E>,
    ) -> Result<Self, E>
    where
        E: From<alloc::Overflow> + From<alloc::Exhausted>,
    {
        let memory = tail::Plan::try_new(entries)?.try_allocate()?;
        Ok(Self::initialize(memory, context()?))
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
        self.scope_in(state, (), run)
    }
}
impl<T, C: buffer::raw::Fixed<T>> Arena<T, C, ()> {
    /// Allocates the exact arena before its context is constructed.
    pub fn try_allocate(
        entries: num::NonZero<u32>,
    ) -> Result<impl FnOnce(C) -> Self, alloc::Error> {
        let memory = tail::Plan::try_new(entries)?.try_allocate()?;
        Ok(move |context| Self::initialize(memory, context))
    }
    pub fn scope<S, R>(
        &mut self,
        state: pin::Pin<&mut S>,
        run: impl for<'d> FnOnce(
            region::Token<region::Identity<'d>>,
            Root<'d, T, C, ()>,
            pin::Pin<&'d mut S>,
        ) -> R,
    ) -> R {
        let mode = self.context().resident();
        self.scope_in(state, mode, run)
    }
}

/// Couples a slot's resident field with its admissible arena configuration.
pub(in crate::buffer::storage::provided) trait Layout {
    type Setting;

    fn mode(setting: Self::Setting) -> provided::Mode;
}

#[repr(C, align(8))]
pub(in crate::buffer::storage::provided) struct Slot<'d, T, L> {
    entry: arena::Entry<'d>,
    resident: L,
    item: cell::UnsafeCell<mem::MaybeUninit<T>>,
}

impl<T> Slot<'_, T, cell::Cell<u32>> {
    pub(in crate::buffer::storage::provided) fn set_resident(&self, resident: u32) {
        self.resident.set(resident);
    }
}

#[repr(transparent)]
pub(in crate::buffer::storage::provided) struct Returned<'d, T, L> {
    node: arena::Ready<'d>,
    domain: marker::PhantomData<&'d Slot<'d, T, L>>,
}

impl<'d, T, L> Returned<'d, T, L> {
    fn entry(&self) -> &Slot<'d, T, L> {
        // SAFETY: this affine ready node retains its original typed slot.
        unsafe { self.node.entry.cast::<Slot<'d, T, L>>().as_ref() }
    }

    fn publish(self, view: buffer::raw::Projection<'d, '_>) -> provided::Unique<'d> {
        let owner = arena::Entry::activate(self);
        provided::Unique::new(view, owner)
    }

    fn reject(self, error: view::LengthError) -> crate::mem::Rejected<T, view::LengthError> {
        crate::mem::Rejected::new(self.take(), error)
    }

    pub(in crate::buffer::storage::provided) fn take(self) -> T {
        // SAFETY: the affine ready node owns this initialized, detached item.
        let item = unsafe { (*self.entry().item.get()).assume_init_read() };
        arena::Entry::recycle(self.node);
        item
    }

    pub(in crate::buffer::storage::provided) fn discard(self) {
        // SAFETY: terminal ownership exclusively owns this initialized item.
        unsafe { (*self.entry().item.get()).assume_init_drop() };
        arena::Entry::recycle(self.node);
    }
}

impl arena::Layout for () {
    type Setting = num::NonZero<u32>;

    fn mode(resident: Self::Setting) -> provided::Mode {
        provided::Mode::Fixed(resident)
    }
}

impl arena::Layout for cell::Cell<u32> {
    type Setting = ();

    fn mode((): Self::Setting) -> provided::Mode {
        provided::Mode::Exact
    }
}

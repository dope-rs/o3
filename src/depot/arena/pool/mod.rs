use std::{cell, marker, mem, ops, pin, ptr};

use crate::{
    alloc::tail,
    depot::{
        arena::pool::{
            self,
            recycle::release,
            shared::{Header, Initialized, Resident},
        },
        capacity,
    },
    mem::{raw::Edge, rc},
};

pub mod recycle;
pub mod shared;

pub struct Vacant<
    'a,
    T,
    O: crate::depot::raw::PoolOwner<T>,
    P: crate::depot::raw::PoolPolicy<T, O> = (),
> {
    entry: ptr::NonNull<pool::Entry<T, O>>,
    owner: &'a O,
    policy: marker::PhantomData<P>,
}

#[repr(transparent)]
pub struct Unique<
    'a,
    T,
    O: crate::depot::raw::PoolOwner<T>,
    P: crate::depot::raw::PoolPolicy<T, O> = (),
    M = (),
> {
    entry: ptr::NonNull<pool::Entry<T, O>>,
    owner: marker::PhantomData<fn(&'a O, P, M) -> &'a O>,
}

#[derive(Debug)]
#[repr(transparent)]
pub struct Shared<'a, T> {
    entry: crate::mem::raw::Owner<pool::Counted<T>>,
    owner: marker::PhantomData<fn(&'a T) -> &'a T>,
}

impl<'a, T, O: crate::depot::raw::PoolOwner<T>, P: crate::depot::raw::PoolPolicy<T, O>>
    Vacant<'a, T, O, P>
{
    pub(crate) fn new(owner: &'a O) -> Option<Self> {
        let free = unsafe { owner.free() };
        let entry = ptr::NonNull::new(free.get())?;
        free.set(unsafe { entry.as_ref().state.get().cast() });
        Some(Self {
            entry,
            owner,
            policy: marker::PhantomData,
        })
    }

    pub fn reserve(self) -> Unique<'a, T, O, P> {
        let this = mem::ManuallyDrop::new(self);
        unsafe {
            this.entry
                .as_ref()
                .state
                .set(ptr::from_ref(this.owner).cast_mut())
        };
        Unique {
            entry: this.entry,
            owner: marker::PhantomData,
        }
    }
}

impl<T, O: crate::depot::raw::PoolOwner<T>, P: crate::depot::raw::PoolPolicy<T, O>> Drop
    for Vacant<'_, T, O, P>
{
    fn drop(&mut self) {
        unsafe { pool::Entry::recycle(self.entry, self.owner.free()) };
    }
}

impl<'a, T, O: crate::depot::raw::PoolOwner<T>, P: crate::depot::raw::PoolPolicy<T, O>, M>
    Unique<'a, T, O, P, M>
{
    pub fn get(&self) -> pin::Pin<&T> {
        // SAFETY: the owned entry is structurally pinned for the retained borrow.
        unsafe { pin::Pin::new_unchecked(&self.entry.as_ref().value) }
    }
}

impl<T, O: crate::depot::raw::PoolOwner<T>, P: crate::depot::raw::PoolMutable<T, O>>
    Unique<'_, T, O, P>
{
    pub fn get_mut(&mut self) -> pin::Pin<&mut T> {
        // SAFETY: the owned entry is structurally pinned for the retained mutable borrow.
        unsafe { pin::Pin::new_unchecked(&mut self.entry.as_mut().value) }
    }
}

impl<'a, T> Unique<'a, Resident<T>, Header<T>, Initialized> {
    pub fn freeze(self) -> Shared<'a, T> {
        // SAFETY: begin/try_unique retain an initialized value with an empty reference domain.
        unsafe { self.get().get_ref().activate() };
        let this = mem::ManuallyDrop::new(self);
        Shared {
            // SAFETY: activation transfers one counted edge to the same transparent slot view.
            entry: unsafe { crate::mem::raw::Owner::from_raw(this.entry.cast()) },
            owner: marker::PhantomData,
        }
    }

    pub fn freeze_pair(self) -> (Shared<'a, T>, Shared<'a, T>) {
        // SAFETY: this unique view owns the empty domain and publishes exactly two edges.
        unsafe { self.get().get_ref().start_pair() };
        let this = mem::ManuallyDrop::new(self);
        (
            Shared {
                // SAFETY: activation transfers one counted edge to the same transparent slot view.
                entry: unsafe { crate::mem::raw::Owner::from_raw(this.entry.cast()) },
                owner: marker::PhantomData,
            },
            Shared {
                // SAFETY: activation transfers one counted edge to the same transparent slot view.
                entry: unsafe { crate::mem::raw::Owner::from_raw(this.entry.cast()) },
                owner: marker::PhantomData,
            },
        )
    }
}

impl<T, O: crate::depot::raw::PoolOwner<T>, P: crate::depot::raw::PoolPolicy<T, O>, M> Drop
    for Unique<'_, T, O, P, M>
{
    fn drop(&mut self) {
        P::retire(pool::Terminal {
            entry: self.entry,
            owner: marker::PhantomData,
        });
    }
}

impl<'a, T> Shared<'a, T> {
    fn get(&self) -> &Resident<T> {
        &self.entry.get().entry.value
    }

    pub fn is_same(&self, other: &Self) -> bool {
        self.entry.as_ptr() == other.entry.as_ptr()
    }

    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            entry: self.entry.try_clone()?,
            owner: marker::PhantomData,
        })
    }

    pub fn try_unique(self) -> Result<Unique<'a, Resident<T>, Header<T>, Initialized>, Self> {
        if !self.get().is_unique() {
            return Err(self);
        }
        unsafe { self.get().retire() };
        Ok(Unique {
            entry: self.entry.into_raw().cast(),
            owner: marker::PhantomData,
        })
    }
}

impl<'owner, T> pool::Unique<'owner, Resident<T>, Header<T>> {
    pub fn begin(mut self, value: T) -> pool::Unique<'owner, Resident<T>, Header<T>, Initialized> {
        let resident = self.get_mut();
        unsafe { resident.value.get().write(mem::MaybeUninit::new(value)) };
        // SAFETY: the exclusive resident now contains exactly one initialized value.
        unsafe { self.map() }
    }

    pub fn commit(self, value: T) -> pool::Shared<'owner, T> {
        self.begin(value).freeze()
    }
}

impl<T> ops::Deref for pool::Unique<'_, Resident<T>, Header<T>, Initialized> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { (*self.get().value.get()).assume_init_ref() }
    }
}

impl<T> ops::DerefMut for pool::Unique<'_, Resident<T>, Header<T>, Initialized> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (*self.get().value.get()).assume_init_mut() }
    }
}

impl<T> ops::Deref for pool::Shared<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { (*self.get().value.get()).assume_init_ref() }
    }
}

impl<'owner, T, C> pool::Unique<'owner, T, release::Header<T, C>, release::Notify<'owner>>
where
    T: recycle::Recycle + 'owner,
    C: release::Context<'owner, T>,
{
    pub fn commit(
        self,
    ) -> pool::Unique<'owner, T, release::Header<T, C>, release::Notify<'owner>, release::Frozen>
    {
        let this = mem::ManuallyDrop::new(self);
        Unique {
            entry: this.entry,
            owner: marker::PhantomData,
        }
    }
}

impl<'a, T, O: crate::depot::raw::PoolOwner<T>, P: crate::depot::raw::PoolPolicy<T, O>, M>
    Unique<'a, T, O, P, M>
{
    /// The value must satisfy the destination policy's initialized-state invariant.
    pub(crate) unsafe fn map<Q: crate::depot::raw::PoolPolicy<T, O>>(
        self,
    ) -> Unique<'a, T, O, Q, M> {
        let this = mem::ManuallyDrop::new(self);
        Unique {
            entry: this.entry,
            owner: marker::PhantomData,
        }
    }
}

/// Physical slot shared by stable-address local pool policies.
#[derive(Debug)]
#[repr(C)]
pub struct Entry<T, O> {
    pub(in crate::depot::arena::pool) value: T,
    pub(in crate::depot::arena::pool) state: cell::Cell<*mut O>,
}

/// Counted view of the same initialized shared-value slot.
#[derive(Debug)]
#[repr(transparent)]
struct Counted<T> {
    entry: Entry<Resident<T>, Header<T>>,
}

// SAFETY: this view retains an initialized value and its original pool owner
// until the final counted edge transfers the slot to its retirement policy.
unsafe impl<T> crate::mem::raw::Target for Counted<T> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        <Initialized as crate::depot::raw::PoolPolicy<Resident<T>, Header<T>>>::retire(Terminal {
            entry: pointer.cast(),
            owner: marker::PhantomData,
        });
    }
}

pub struct Terminal<'a, T, O: crate::depot::raw::PoolOwner<T>> {
    pub(in crate::depot::arena::pool) entry: ptr::NonNull<Entry<T, O>>,
    pub(in crate::depot::arena::pool) owner: marker::PhantomData<&'a O>,
}

impl<'a, T, O: crate::depot::raw::PoolOwner<T>> Terminal<'a, T, O> {
    pub fn get_mut(&mut self) -> pin::Pin<&mut T> {
        // SAFETY: the terminal entry is structurally pinned for the retained mutable borrow.
        unsafe { pin::Pin::new_unchecked(&mut self.entry.as_mut().value) }
    }

    pub(crate) fn recycle(self) -> &'a O {
        // SAFETY: this terminal entry retains its initialized owner's original loan.
        let owner = unsafe { &*self.entry.as_ref().state.get() };
        unsafe { Entry::recycle(self.entry, owner.free()) };
        owner
    }
}

impl<T, O> Entry<T, O> {
    pub(crate) fn init(
        memory: tail::Uninit<O, Self, capacity::Count>,
        header: O,
        mut init: impl FnMut(u32) -> T,
    ) -> tail::Block<O, Self, capacity::Count>
    where
        O: crate::depot::raw::PoolOwner<T>,
    {
        let mut block = memory.init(header);
        let indices = 0..block.extent().raw();
        let (header, entries) = block.parts_mut();
        let mut free: *mut Self = ptr::null_mut();
        for (index, slot) in indices.zip(entries) {
            free = ptr::from_mut(slot.write(Self {
                value: init(index),
                state: cell::Cell::new(free.cast()),
            }));
        }
        // SAFETY: each initialized slot belongs to this retained pinned owner.
        unsafe { header.free() }.set(free);
        // SAFETY: every admitted slot was initialized once in index order.
        unsafe { block.assume_init() }
    }

    pub(in crate::depot::arena::pool) unsafe fn recycle(
        entry: ptr::NonNull<Self>,
        free: &cell::Cell<*mut Self>,
    ) {
        unsafe {
            entry
                .as_ref()
                .state
                .set(free.replace(entry.as_ptr()).cast())
        };
    }
}

// SAFETY: the transparent view preserves the initialized entry's exact reference domain.
unsafe impl<T> crate::mem::raw::Edge for Counted<T> {
    fn count(&self) -> &crate::mem::raw::Count {
        self.entry.value.count()
    }
}

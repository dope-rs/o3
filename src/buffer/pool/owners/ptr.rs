use std::{marker, mem, num, ptr};

use crate::{
    alloc,
    buffer::{pool, pool::state, resident, storage::regions},
};

/// Physical layout of an accounted block before its resident domain is bound.
pub(in crate::buffer::pool) struct Accounted;

#[derive(Debug)]
pub(in crate::buffer::pool) struct Any;

/// One allocated pool block that has not published a live core capability.
pub(in crate::buffer::pool) struct Block<K, S> {
    base: Base<K>,
    shape: super::Shape<K>,
    state: marker::PhantomData<S>,
}

/// An allocation address that carries no initialized-header capability.
#[repr(transparent)]
#[derive(Debug)]
pub(super) struct Base<K>(ptr::NonNull<super::Header<K>>);

impl<K> Base<K> {
    pub(super) const unsafe fn from_raw(raw: ptr::NonNull<super::Header<K>>) -> Self {
        Self(raw)
    }

    pub(super) const fn as_raw(&self) -> ptr::NonNull<super::Header<K>> {
        self.0
    }

    pub(super) unsafe fn slots(self) -> ptr::NonNull<super::Slot<K>>
    where
        K: crate::buffer::raw::Backing,
    {
        // SAFETY: callers prove this base has storage for the Header followed
        // by the Slot array selected by its physical layout.
        unsafe { self.0.add(1).cast() }
    }

    pub(super) const unsafe fn publish<S>(self) -> Ptr<K, S> {
        Ptr {
            raw: self.0,
            kind: marker::PhantomData,
        }
    }
}

impl<K> Clone for Base<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Base<K> {}

impl<'d> Base<super::Resident<'d>> {
    pub(super) unsafe fn accounts(self, slots: u32) -> ptr::NonNull<super::Accounts<'d>> {
        // SAFETY: callers prove this is an accounted layout with exactly this
        // many Slot cells before its Accounts table.
        unsafe { self.slots().add(slots as usize).cast() }
    }
}

impl<'d> Base<super::Charged<'d>> {
    unsafe fn budget(self, slots: u32) -> ptr::NonNull<resident::Budget<'d>> {
        // SAFETY: the charged shape places its aligned budget after these slots.
        unsafe { self.slots().add(slots as usize).cast() }
    }
}

impl<K, S: state::State> Block<K, S> {
    #[inline(always)]
    pub(super) fn allocate(shape: super::Shape<K>) -> Result<Self, alloc::Exhausted> {
        let (_, layout) = shape.allocation();
        // SAFETY: Shape proves a valid nonzero allocation including its Header.
        let block = unsafe { alloc::raw::try_new(layout, S::ZEROED) }?;
        // SAFETY: this exact shape supplies both allocation and initialization geometry.
        let base = unsafe { Base::from_raw(block) };
        // SAFETY: this unpublished allocation owns its exact shape's Header cell.
        unsafe { base.as_raw().write(super::Header::new(shape)) };
        Ok(Self {
            base,
            shape,
            state: marker::PhantomData,
        })
    }
}

impl<L: Into<pool::Layout>, S: state::State> Block<super::Plain<L>, S>
where
    super::Plain<L>: crate::buffer::raw::Backing<Link = ()>,
{
    pub(in crate::buffer::pool) fn try_new(
        public: L,
    ) -> Result<Ptr<super::Plain<L>, S>, alloc::Exhausted> {
        let block = Self::allocate(super::Shape::new(public))?;
        // SAFETY: this plain shape owns exactly these uninitialized slots.
        unsafe { super::Slot::initialize(block.base, block.shape.slots(), ()) };
        // SAFETY: the complete plain block has been initialized before publication.
        Ok(unsafe { block.base.publish() })
    }
}

impl<S: state::State> Block<super::Plain<pool::Layout>, S> {
    pub(in crate::buffer::pool) fn try_seed(
        public: pool::NonEmpty,
    ) -> Result<super::Seed<super::Plain<pool::Layout>, S>, alloc::Exhausted> {
        let core = Self::try_new(public.into_layout())?;
        // SAFETY: NonEmpty proves this freshly initialized list has a first slot.
        Ok(unsafe { super::Seed::from_ptr(core) })
    }
}

impl<S: state::State> Block<Accounted, S> {
    pub(in crate::buffer::pool) fn try_seed<'d>(
        public: pool::NonEmpty,
        budget: resident::Budget<'d>,
    ) -> Result<super::Seed<super::Resident<'d>, S>, resident::AllocError> {
        let core = Self::try_new(public.into(), budget)?;
        // SAFETY: NonEmpty proves this freshly published list has a first slot.
        Ok(unsafe { super::Seed::from_ptr(core) })
    }

    pub(in crate::buffer::pool) fn try_new<'d>(
        tally: pool::Tally,
        budget: resident::Budget<'d>,
    ) -> Result<Ptr<super::Resident<'d>, S>, resident::AllocError> {
        let shape =
            super::Shape::<Accounted>::try_new(tally).map_err(resident::AllocError::Layout)?;
        let (layout, limit) = tally.into_parts();
        let charge = budget
            .try_charge(layout.bytes())
            .map_err(resident::AllocError::Budget)?;
        let block = Self::allocate(shape).map_err(resident::AllocError::Exhausted)?;
        // SAFETY: shape and charge retain this tally's account count and payload capacity.
        Ok(unsafe { block.account(charge, limit) })
    }

    /// # Safety
    /// `limit` is this shape's account count; `charge` covers its payload capacity.
    #[inline]
    pub(super) unsafe fn account<'d>(
        self,
        charge: resident::Charge<'d>,
        limit: u32,
    ) -> Ptr<super::Resident<'d>, S> {
        let slots = self.shape.slots();
        // SAFETY: the caller retains the shape's account count and complete charge.
        let base = unsafe { super::Accounts::initialize(self.base, slots, charge, limit) };
        // SAFETY: all cells are initialized and the transferred charge binds the published domain.
        unsafe { base.publish() }
    }
}

impl<'d> Block<super::Charged<'d>, state::Uninit> {
    pub(in crate::buffer::pool) fn try_charged(
        public: pool::Layout,
        budget: resident::Budget<'d>,
    ) -> Result<Ptr<super::Charged<'d>>, resident::AllocError> {
        let shape = super::Shape::<super::Charged<'d>>::try_new(public)
            .map_err(resident::AllocError::Layout)?;
        let charge = budget
            .try_charge(public.bytes())
            .map_err(resident::AllocError::Budget)?;
        let block = Self::allocate(shape).map_err(resident::AllocError::Exhausted)?;
        // SAFETY: this unpublished shape reserves exactly these unassociated slots.
        unsafe { super::Slot::initialize(block.base, public.slots(), ()) };
        let charge = mem::ManuallyDrop::new(charge);
        // SAFETY: transfer the exact shape's debit before publishing any owner.
        unsafe {
            block.base.budget(public.slots()).write(charge.budget());
            Ok(block.base.publish())
        }
    }
}

#[repr(transparent)]
#[derive(Debug)]
pub struct Ptr<K, S = state::Uninit> {
    raw: ptr::NonNull<super::Header<K>>,
    kind: marker::PhantomData<(K, S)>,
}

impl<K, S> Ptr<K, S> {
    pub(super) const fn as_raw(&self) -> ptr::NonNull<super::Header<K>> {
        self.raw
    }

    pub(super) fn header(&self) -> &super::Header<K> {
        // SAFETY: Ptr retains one published pool header for every state it can
        // hold, and this shared borrow cannot outlive that retention.
        unsafe { self.raw.as_ref() }
    }

    pub(in crate::buffer::pool) fn available(self) -> usize {
        self.header().available.get() as usize
    }

    pub(in crate::buffer::pool) const fn erase_state(self) -> Ptr<K, pool::Frozen> {
        Ptr {
            raw: self.raw,
            kind: marker::PhantomData,
        }
    }
}

impl<'d, S> Ptr<super::Charged<'d>, S> {
    pub(in crate::buffer) unsafe fn destroy(self) {
        let (bytes, layout) = self.header().allocation();
        // SAFETY: the terminal edge retains this initialized charged block.
        let budget = unsafe {
            Base::from_raw(self.raw)
                .budget(self.header().slots())
                .read()
        };
        // SAFETY: immutable geometry retains this root's exact debit; no owner remains.
        unsafe {
            budget.release(bytes);
            alloc::raw::dealloc(layout, self.raw);
        }
    }
}

impl<'d, S> Ptr<super::Resident<'d>, S> {
    pub(super) const fn base(self) -> Base<super::Resident<'d>> {
        Base(self.raw)
    }
}

impl<K, S> Clone for Ptr<K, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, S> Copy for Ptr<K, S> {}

const _: () = {
    assert!(size_of::<Base<super::Plain>>() == size_of::<usize>());
    assert!(size_of::<Base<Accounted>>() == size_of::<usize>());
    assert!(size_of::<Base<super::Resident<'static>>>() == size_of::<usize>());
    assert!(
        size_of::<Block<super::Plain<pool::Fixed<1, 1>>, state::Uninit>>()
            == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Block<super::Plain<pool::Layout>, state::Uninit>>() == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Block<super::Plain<pool::Width<1>>, state::Uninit>>() == 3 * size_of::<usize>()
    );
    assert!(size_of::<Block<Accounted, state::Uninit>>() == 3 * size_of::<usize>());
    assert!(size_of::<Ptr<super::Plain>>() == size_of::<usize>());
    assert!(size_of::<Ptr<super::Resident<'static>>>() == size_of::<usize>());
    assert!(size_of::<Ptr<Accounted>>() == size_of::<usize>());
};

/// Reconstructed pool and slot identity carried by one live storage owner.
pub(super) type Binding<K> = (super::Ptr<K, super::Any>, super::Key);

impl<K: crate::buffer::raw::Backing, S> Ptr<K, S> {
    pub(in crate::buffer) unsafe fn deallocate(self) {
        let (_, layout) = super::Shape::from_ptr(self).allocation();
        // SAFETY: terminal ownership retains this allocation's admitted geometry
        // through the exact matching foreign allocator boundary.
        unsafe { alloc::raw::dealloc(layout, self.raw) };
    }

    pub(in crate::buffer::pool) fn capacity(self) -> num::NonZero<u32> {
        match K::CAPACITY {
            Some(capacity) => capacity,
            None => self.header().capacity(),
        }
    }

    pub(in crate::buffer::pool) fn slots(self) -> usize {
        match K::SLOTS {
            Some(slots) => slots as usize,
            None => self.header().slots() as usize,
        }
    }

    pub(super) fn data_offset(self) -> num::NonZero<usize> {
        match K::SLOTS {
            Some(slots) => pool::Layout::offset(slots),
            None => self.header().data_offset(),
        }
    }

    pub(super) fn is_unique(&self, key: super::Key) -> bool {
        use crate::mem::raw::Edge;

        self.slot(key).is_unique()
    }

    pub(super) fn data(self, key: super::Key) -> *mut u8 {
        // SAFETY: Backing preserves the admitted geometry and Key retains this
        // pool-bound slot range, whether its dimensions are static or stored.
        unsafe {
            self.as_raw()
                .as_ptr()
                .cast::<u8>()
                .add(self.data_offset().get() + key.get() as usize * self.capacity().get() as usize)
        }
    }

    pub(super) fn owner(self, key: super::Key) -> ptr::NonNull<regions::Prefix> {
        self.ptr(key).cast()
    }

    pub(super) fn ptr(self, key: super::Key) -> ptr::NonNull<super::Slot<K>> {
        // SAFETY: this core retains the published header whose admitted layout
        // owns the Slot array directly after it, and Key is issued from this
        // core and bounded by header.slots.
        unsafe {
            super::Base::from_raw(self.as_raw())
                .slots()
                .add(key.get() as usize)
        }
    }

    pub(super) fn slot(&self, key: super::Key) -> &super::Slot<K> {
        // SAFETY: Key retains the exact list proof and tying the reference to
        // `self` prevents it from escaping the capability borrow.
        unsafe { self.ptr(key).as_ref() }
    }
}

impl<K: crate::buffer::raw::Backing> Ptr<K, Any> {
    pub(super) unsafe fn from_owner(owner: ptr::NonNull<()>) -> Binding<K> {
        let slot = owner.cast::<super::Slot<K>>();
        // SAFETY: storage owners are created only from Slot::prefix addresses.
        let key = unsafe { slot.as_ref().key() };
        let offset = size_of::<super::Header>() + key.get() as usize * size_of::<super::Slot<K>>();
        // SAFETY: the stored slot index reconstructs the allocation base used
        // by Layout without leaving that allocation.
        let raw = unsafe { slot.cast::<u8>().sub(offset).cast() };
        // SAFETY: an owner edge keeps this fully initialized reconstructed
        // header allocation live, and the caller carries its original kind.
        let base = unsafe { super::Base::from_raw(raw) };
        // SAFETY: the live typed owner edge proves publication completed before
        // this state-erased reconstruction of the same backing kind.
        let core = unsafe { base.publish() };
        (core, key)
    }
}

use std::{marker, num, ptr};

use crate::{
    alloc,
    buffer::{
        pool,
        pool::{owners, state},
        resident,
        storage::regions,
    },
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

    pub(super) unsafe fn slots(self) -> ptr::NonNull<super::Slot> {
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

impl<K, S: state::State> Block<K, S> {
    fn allocate(shape: super::Shape<K>) -> Result<Self, alloc::Exhausted> {
        let layout = shape.allocation();
        // SAFETY: Shape proves a valid nonzero allocation including its Header.
        let block = unsafe { alloc::raw::try_new(layout, S::ZEROED) }?;
        // SAFETY: this exact shape supplies both allocation and initialization geometry.
        let base = unsafe { Base::from_raw(block) };
        Ok(Self {
            base,
            shape,
            state: marker::PhantomData,
        })
    }

    fn initialize(&self) {
        let header = super::Header::new(self.shape);
        // SAFETY: Block uniquely owns the Header cell allocated by this same shape.
        unsafe { self.base.as_raw().write(header) };
        // SAFETY: the same shape owns exactly these uninitialized Slot cells.
        unsafe { super::Slot::initialize(self.base, self.shape.slots()) };
    }
}

impl<L, S: state::State> Block<super::Plain<L>, S> {
    fn init(self) -> Ptr<super::Plain<L>, S> {
        self.initialize();
        // SAFETY: the complete plain block has been initialized before publication.
        unsafe { self.base.publish() }
    }
}

impl<L: Into<pool::Layout>, S: state::State> Block<super::Plain<L>, S> {
    pub(in crate::buffer::pool) fn try_new(
        public: L,
    ) -> Result<Ptr<super::Plain<L>, S>, alloc::Exhausted> {
        Ok(Self::allocate(super::Shape::new(public))?.init())
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
        let shape = super::Shape::try_new(tally).map_err(resident::AllocError::Layout)?;
        let (layout, limit) = tally.into_parts();
        let charge = budget
            .try_charge(layout.bytes())
            .map_err(resident::AllocError::Budget)?;
        let block = Self::allocate(shape).map_err(resident::AllocError::Exhausted)?;
        let slots = block.shape.slots();
        block.initialize();
        // SAFETY: the same shape reserves exactly the admitted tally's account extent.
        let base = unsafe { super::Accounts::initialize(block.base, slots, charge, limit) };
        // SAFETY: all cells are initialized and the transferred charge binds the published domain.
        Ok(unsafe { base.publish() })
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

    pub(in crate::buffer::pool) fn capacity(self) -> num::NonZero<u32> {
        self.header().capacity()
    }

    pub(in crate::buffer::pool) fn slots(self) -> usize {
        self.header().slots() as usize
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

    pub(in crate::buffer) unsafe fn deallocate(self) {
        // SAFETY: terminal ownership pairs this allocation with its immutable
        // nonzero geometry, consumed directly at the foreign allocator boundary.
        unsafe { alloc::raw::dealloc(self.header().allocation(), self.raw) };
    }
}

impl<'d, S> Ptr<super::Resident<'d>, S> {
    /// # Safety
    /// The consumed core is the terminal edge of this exact resident allocation.
    pub(in crate::buffer) unsafe fn uncharge(self) {
        unsafe { owners::Accounts::release(self) };
    }

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
pub(super) struct Binding<K> {
    pub(super) slots: super::Ptr<K, super::Any>,
    pub(super) key: super::Key,
}

impl<K, S> Ptr<K, S> {
    pub(super) fn is_unique(&self, key: super::Key) -> bool {
        use crate::mem::raw::Edge;

        self.slot(key).is_unique()
    }

    pub(super) fn data(self, key: super::Key) -> *mut u8 {
        let header = self.header();
        // SAFETY: Layout reserves `capacity` bytes for every valid slot at the
        // stored data offset; Key retains this pool-bound range proof.
        unsafe {
            self.as_raw().as_ptr().cast::<u8>().add(
                header.data_offset().get() + key.get() as usize * header.capacity().get() as usize,
            )
        }
    }

    pub(super) fn owner(self, key: super::Key) -> ptr::NonNull<regions::Prefix> {
        self.ptr(key).cast()
    }

    pub(super) fn ptr(self, key: super::Key) -> ptr::NonNull<super::Slot> {
        // SAFETY: this core retains the published header whose admitted layout
        // owns the Slot array directly after it, and Key is issued from this
        // core and bounded by header.slots.
        unsafe {
            super::Base::from_raw(self.as_raw())
                .slots()
                .add(key.get() as usize)
        }
    }

    pub(super) fn slot(&self, key: super::Key) -> &super::Slot {
        // SAFETY: Key retains the exact list proof and tying the reference to
        // `self` prevents it from escaping the capability borrow.
        unsafe { self.ptr(key).as_ref() }
    }
}

impl<K> Ptr<K, Any> {
    pub(super) unsafe fn from_owner(owner: ptr::NonNull<()>) -> Binding<K> {
        let slot = owner.cast::<super::Slot>();
        // SAFETY: storage owners are created only from Slot::prefix addresses.
        let key = unsafe { slot.as_ref() }.key();
        let offset = size_of::<super::Header>() + key.get() as usize * size_of::<super::Slot>();
        // SAFETY: the stored slot index reconstructs the allocation base used
        // by Layout without leaving that allocation.
        let raw = unsafe { slot.cast::<u8>().sub(offset).cast() };
        // SAFETY: an owner edge keeps this fully initialized reconstructed
        // header allocation live, and the caller carries its original kind.
        let base = unsafe { super::Base::from_raw(raw) };
        // SAFETY: the live typed owner edge proves publication completed before
        // this state-erased reconstruction of the same backing kind.
        let core = unsafe { base.publish() };
        Binding { slots: core, key }
    }
}

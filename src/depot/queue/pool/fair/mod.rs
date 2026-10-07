//! Region-owned FIFO lanes with fair unit admission embedded in their sidecars.

use std::{cell, mem, num};

use credit::fair;
pub use lanes::Lanes;

use crate::{
    base::cell::region,
    depot::{
        arena::{linked, linked::slots},
        capacity,
        queue::pool,
    },
    mem::{Lease, Return, credit, lease},
};

mod lanes;

#[derive(Clone, Copy, Eq, PartialEq)]
pub struct Plan {
    slots: capacity::Count,
    lanes: num::NonZero<u32>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum Error {
    ZeroLanes { capacity: usize },
    CapacityBelowLanes { capacity: usize, lanes: usize },
    Capacity(capacity::Error),
}

const _: () = assert!(std::mem::size_of::<Plan>() == 2 * std::mem::size_of::<u32>());

impl Plan {
    pub fn new(capacity: usize, lanes: usize) -> Result<Self, Error> {
        if capacity < lanes {
            return Err(Error::CapacityBelowLanes { capacity, lanes });
        }
        let Some(count) = num::NonZero::<u32>::new(lanes as u32) else {
            return Err(if lanes == 0 {
                Error::ZeroLanes { capacity }
            } else {
                Error::Capacity(capacity::Error::new(capacity))
            });
        };
        let slots = capacity::Count::try_from(capacity).map_err(Error::Capacity)?;
        Ok(Self {
            slots,
            lanes: count,
        })
    }
}

pub struct State<S> {
    value: cell::UnsafeCell<S>,
    held: cell::Cell<capacity::Count>,
    index: capacity::Count,
}

impl<S: Default> Default for State<S> {
    fn default() -> Self {
        Self {
            value: cell::UnsafeCell::new(S::default()),
            held: cell::Cell::new(capacity::Count::EMPTY),
            index: capacity::Count::EMPTY,
        }
    }
}

impl<S> State<S> {
    pub const fn value(&self) -> &S {
        // SAFETY: Lane retains the shared region borrow that excludes exclusive value access.
        unsafe { &*self.value.get() }
    }
}

/// One fair lane retaining its exact owner and exclusive region brand.
pub struct Binding<'a, 'd, T, S = ()> {
    owner: &'a Lanes<'d, T, S>,
    slot: &'a cell::UnsafeCell<crate::depot::raw::QueueSlot<State<S>>>,
}

impl<'a, 'd, T, S> Binding<'a, 'd, T, S> {
    /// Projects an admitted lane while retaining its exact owner and region.
    pub fn from_source(
        source: crate::depot::raw::Proof<(&'a Lanes<'d, T, S>, usize), capacity::Physical>,
    ) -> Self {
        let (owner, index) = source.into_inner();
        // SAFETY: the retained physical proof bounds this index in the original
        // owner, whose fixed lanes stay initialized for its complete borrow.
        let slot = unsafe { &*owner.inner.slots.as_ptr().add(index).cast() };
        Self { owner, slot }
    }

    pub const fn index(self) -> usize {
        // SAFETY: bind retains this initialized slot's immutable original index.
        unsafe { (*self.slot.get()).state.index.get() }
    }

    pub fn view<'t>(
        self,
        _: &'t region::Token<region::Identity<'d>>,
    ) -> pool::View<'t, T, State<S>, false>
    where
        'a: 't,
    {
        // SAFETY: the matching shared token excludes exclusive lane loans for 't.
        unsafe { pool::View::new(&self.owner.inner.store, &*self.slot.get()) }
    }

    fn lane_mut<'t>(
        self,
        _: &'t mut region::Token<region::Identity<'d>>,
    ) -> pool::Lane<'t, T, State<S>, false>
    where
        'a: 't,
    {
        // SAFETY: the matching exclusive token excludes every competing lane
        // loan; only fair transitions may consume slots or access the ledger.
        unsafe { pool::Lane::new(&self.owner.inner.store, &mut *self.slot.get()) }
    }

    fn state(self) -> &'a State<S> {
        // SAFETY: fair admission touches only the Cell ledger; value loans stay
        // inside its disjoint UnsafeCell and require the matching region token.
        unsafe { &(*self.slot.get()).state }
    }

    pub fn reserve(self) -> Result<Permit<'a, 'd, T, S>, fair::Cause<usize>> {
        let state = self.state();
        let held = state.held.get();
        let shared = self.owner.shared.get();
        let balance = crate::mem::raw::Balance::new([held.raw()], [shared.raw()]);
        let [required] = balance.demand([1], [1]);
        if required > shared.raw() {
            return Err(fair::Cause::Exhausted {
                axis: fair::Axis::at::<0>(),
                required: 1,
                available: 0,
            });
        }
        // SAFETY: each lane retains its first slot, and the remaining shared
        // demand was admitted above. Total storage fits u32, so success cannot
        // overflow held; both counters still describe the original allocation.
        let balance = unsafe { balance.acquire_raw([1], [1]) };
        let ([held], [shared]) = balance.into_parts();
        state.held.set(capacity::Count::new(held));
        self.owner.shared.set(capacity::Count::new(shared));
        Ok(Permit::new(self))
    }

    pub fn take_front(
        self,
        token: &mut region::Token<region::Identity<'d>>,
    ) -> Option<(T, Permit<'a, 'd, T, S>)> {
        let lane = self.lane_mut(token);
        let index = lane.slot.queue.front_index()?;
        // SAFETY: the original Binding owns this lane's unchanged live front.
        let (value, vacant) = unsafe { lane.slot.queue.take(self, index) };
        Some((value, Permit::new(vacant.release())))
    }
}

impl<T, S> Copy for Binding<'_, '_, T, S> {}

impl<T, S> Clone for Binding<'_, '_, T, S> {
    fn clone(&self) -> Self {
        *self
    }
}

// SAFETY: Binding retains the same fair allocation for its complete lifetime.
impl<T, S> linked::Sealed<T> for Binding<'_, '_, T, S> {}

impl<T, S> linked::Source<T> for Binding<'_, '_, T, S> {
    const TRACK: bool = false;

    fn store(&self) -> &slots::Store<T> {
        &self.owner.inner.store
    }
}

/// One unused queue slot entitlement bound to its original fair lane.
#[must_use = "a fair queue reservation must be published or returned"]
pub struct Permit<'a, 'd, T, S = ()> {
    lease: Lease<Refund<'a, 'd, T, S>, lease::Open>,
}

struct Refund<'a, 'd, T, S> {
    binding: Binding<'a, 'd, T, S>,
}

impl<'a, 'd, T, S> Permit<'a, 'd, T, S> {
    fn new(binding: Binding<'a, 'd, T, S>) -> Self {
        Self {
            lease: Lease::open(Refund { binding }, ()),
        }
    }

    pub fn state_mut<'b>(
        &'b self,
        token: &'b mut region::Token<region::Identity<'d>>,
    ) -> &'b mut S {
        self.lease
            .sink()
            .binding
            .lane_mut(token)
            .into_state()
            .value
            .get_mut()
    }

    pub fn push_back(self, token: &mut region::Token<region::Identity<'d>>, value: T) {
        self.into_claim(token).append(value);
    }

    /// Restores a deferred head using its retained fair-lane entitlement.
    pub fn push_front(self, token: &mut region::Token<region::Identity<'d>>, value: T) {
        crate::mem::Return::return_item(self.into_claim(token), value);
    }

    fn into_claim<'t>(
        self,
        token: &'t mut region::Token<region::Identity<'d>>,
    ) -> slots::Vacant<T, slots::Bound<'t, T, (), false>>
    where
        'a: 't,
    {
        let (Refund { binding }, ()) = self.lease.into_parts();
        let lane = binding.lane_mut(token);
        // SAFETY: the exclusive token retains the original fair lane and its
        // untracked slot policy; the fair ledger owns this affine entitlement.
        let owner = unsafe { lane.slot.queue.bind(lane.store) };
        // SAFETY: this affine permit owns one unused slot in this exact allocation.
        unsafe { slots::Vacant::reserve(owner) }
    }
}

impl<T, S> Return for Refund<'_, '_, T, S> {
    type Item = ();

    fn return_item(self, (): ()) {
        let binding = self.binding;
        let state = binding.state();
        let held = state.held.get();
        let shared = binding.owner.shared.get();
        // SAFETY: this affine refund owns exactly one held unit in the original
        // lane. Returning it cannot exceed the allocation's shared capacity.
        let balance = unsafe {
            crate::mem::raw::Balance::new([held.raw()], [shared.raw()]).release_raw([1], [1])
        };
        let ([held], [shared]) = balance.into_parts();
        state.held.set(capacity::Count::new(held));
        binding.owner.shared.set(capacity::Count::new(shared));
    }
}

const _: () = {
    const WORD: usize = mem::size_of::<usize>();

    assert!(mem::size_of::<State<u64>>() == 2 * WORD);
    assert!(mem::size_of::<Binding<'static, 'static, u64>>() == 2 * WORD);
    assert!(mem::size_of::<Permit<'static, 'static, u64>>() == 2 * WORD);
    assert!(mem::size_of::<Option<Permit<'static, 'static, u64>>>() == 2 * WORD);
};

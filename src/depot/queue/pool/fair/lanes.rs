use std::{cell, marker, num::NonZero};

use crate::{
    alloc,
    base::cell::region,
    depot::{capacity, queue::pool::fair, raw::QueueBlock},
};

pub struct Lanes<'d, T, S = ()> {
    pub(super) inner: QueueBlock<T, fair::State<S>, NonZero<u32>>,
    pub(super) shared: cell::Cell<capacity::Count>,
    domain: marker::PhantomData<*mut &'d region::Token<region::Identity<'d>>>,
}

impl<'d, T, S> Lanes<'d, T, S> {
    pub fn try_new(
        plan: fair::Plan,
        region::Token { .. }: &region::Token<region::Identity<'d>>,
    ) -> Result<Self, alloc::Error>
    where
        S: Default,
    {
        let shared = capacity::Count::new(plan.slots.raw() - plan.lanes.get());
        let inner = QueueBlock::try_with(plan.slots, plan.lanes, |index| fair::State {
            index: capacity::Count::new(index.raw()),
            ..fair::State::default()
        })?;
        Ok(Self {
            inner,
            shared: cell::Cell::new(shared),
            domain: marker::PhantomData,
        })
    }

    /// The admitted nonzero extent of this owner's immutable lane domain.
    pub const fn capacity(&self) -> NonZero<u32> {
        self.inner.count()
    }

    pub fn bind(&self, lane: usize) -> Option<fair::Binding<'_, 'd, T, S>> {
        if lane >= self.capacity().get() as usize {
            return None;
        }
        // SAFETY: admission bounds this lane in its exact immutable owner.
        Some(fair::Binding::from_source(unsafe {
            crate::depot::raw::Proof::new((self, lane))
        }))
    }
}

const _: () =
    assert!(std::mem::size_of::<Lanes<'static, u64>>() == 6 * std::mem::size_of::<usize>());

//! Indexed priorities coupled to resident phases and exclusive event queues.

use std::marker;

use crate::{
    alloc::{self, BoxSliceExt as _},
    depot::{
        capacity,
        heap::{self, indexed},
        queue::{indexed::planes, links},
    },
};

/// Payloads owned by the resident's scheduling and event phases.
pub trait Values {
    type Common;
    type Scheduled;
    type Idle;
    type Control;
    type Data;
    type Active;
}

/// A resident's one initialized field.
pub enum Value<S, I, C, D, A> {
    Scheduled(S),
    Idle(I),
    Control(C),
    Data(D),
    Active(A),
}

/// Owned phase payloads from one resident family.
pub type Owned<T> = Value<
    <T as Values>::Scheduled,
    <T as Values>::Idle,
    <T as Values>::Control,
    <T as Values>::Data,
    <T as Values>::Active,
>;

/// Phase payloads borrowed from the same resident owner.
pub type Borrowed<'a, T> = Value<
    &'a <T as Values>::Scheduled,
    &'a <T as Values>::Idle,
    &'a <T as Values>::Control,
    &'a <T as Values>::Data,
    &'a <T as Values>::Active,
>;

/// The minimum priority and its resident under one shared owner loan.
pub type Peek<'a, P, T> = (
    usize,
    &'a P,
    &'a <T as Values>::Common,
    Option<&'a <T as Values>::Scheduled>,
);

/// A resident phase retaining its exact vacant heap position.
pub type Vacant<'a, P, T, F, C = capacity::Count> = Slot<'a, P, T, heap::Direct, F, C>;

/// A sealed resident field selected by its exclusive owner.
pub trait Phase<T: Values>: Sealed {
    type Item;
    fn value(value: Self::Item) -> Owned<T>;
}

pub mod phase;
mod retained;
pub(crate) use retained::Retained;
mod sealed;
pub(crate) use sealed::Sealed;

pub struct Min<P: Ord, T: Values, C: alloc::raw::Extent = capacity::Count> {
    core: raw::Core<T>,
    order: indexed::Min<P, C>,
}

/// Exact heap membership and resident phase retained in one exclusive loan.
pub struct Slot<'a, P: Ord, T: Values, H, F: Phase<T>, C: alloc::raw::Extent = capacity::Count> {
    order: &'a mut indexed::Min<P, C>,
    core: &'a mut raw::Core<T>,
    index: links::Index,
    state: H,
    field: marker::PhantomData<fn() -> F>,
}

pub enum Control<'a, P: Ord, T: Values, C: alloc::raw::Extent = capacity::Count> {
    Full(Slot<'a, P, T, usize, phase::Control, C>),
    Empty(Vacant<'a, P, T, phase::Control, C>),
}

pub enum Entry<'a, P: Ord, T: Values, C: alloc::raw::Extent = capacity::Count> {
    Scheduled(Slot<'a, P, T, usize, phase::Scheduled, C>),
    Idle(Vacant<'a, P, T, phase::Idle, C>),
    Control(Control<'a, P, T, C>),
    Data(Vacant<'a, P, T, phase::Data, C>),
    Active(Vacant<'a, P, T, phase::Active, C>),
}

impl<P: Ord, T: Values, C: alloc::raw::Extent> Min<P, T, C> {
    /// Starts every represented resident scheduled at the same priority.
    pub fn try_repeat(
        capacity: C,
        priority: P,
        mut make: impl FnMut(usize) -> (T::Common, T::Scheduled),
    ) -> Result<Self, planes::Error<C>>
    where
        P: Copy,
    {
        let order = indexed::Min::try_repeat(capacity, priority).map_err(planes::Error::Heap)?;
        let nodes = Box::try_from_fn(capacity.len(), |index| {
            let (common, value) = make(index);
            raw::Node::scheduled(index, common, value)
        })
        .map_err(|error| planes::Error::Values { capacity, error })?;
        Ok(Self {
            order,
            core: raw::Core::new(nodes),
        })
    }

    pub fn capacity(&self) -> usize {
        self.core.nodes.len()
    }

    pub fn active(&self) -> usize {
        self.core.active as usize
    }

    pub fn has_events(&self) -> bool {
        !self.core.control.is_empty() || !self.core.data.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<(&T::Common, Borrowed<'_, T>)> {
        raw::view(self, index)
    }

    /// A scheduled root is blocked while its control event is still pending.
    pub fn peek(&self) -> Option<Peek<'_, P, T>> {
        raw::peek(self)
    }

    pub fn common_mut(&mut self, index: usize) -> Option<&mut T::Common> {
        Some(&mut self.core.nodes.get_mut(index)?.common)
    }

    pub fn entry(&mut self, index: usize) -> Option<Entry<'_, P, T, C>> {
        raw::entry(self, index)
    }

    pub fn control(&mut self) -> Option<Control<'_, P, T, C>> {
        raw::control(self)
    }

    pub fn data(&mut self) -> Option<Vacant<'_, P, T, phase::Data, C>> {
        raw::data(self)
    }

    /// Retires every priority in linear time, preserving pending event order.
    pub fn clear(&mut self, convert: impl FnMut(&mut T::Common, T::Scheduled) -> T::Idle) {
        if !self.order.is_empty() {
            raw::clear(self, convert);
        }
    }
}

impl<P: Ord, T: Values, C: alloc::raw::Extent> Drop for Min<P, T, C> {
    fn drop(&mut self) {
        raw::destroy(self);
    }
}

pub mod raw;

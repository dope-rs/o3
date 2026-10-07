//! Indexed priorities coupled to resident phases and exclusive event queues.

use crate::{
    alloc::{self, BoxSliceExt as _},
    depot::{capacity, heap::indexed, queue::indexed::planes},
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
pub type Vacant<'a, P, T, F, C = capacity::Count> = Slot<'a, T, indexed::Vacant<'a, P, C>, F>;

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
pub struct Slot<'a, T: Values, H, F: Phase<T>> {
    order: H,
    field: raw::Held<'a, T, F>,
}

pub enum Control<'a, P: Ord, T: Values, C: alloc::raw::Extent = capacity::Count> {
    Full(Slot<'a, T, indexed::Occupied<'a, P, C>, phase::Control>),
    Empty(Vacant<'a, P, T, phase::Control, C>),
}

pub enum Entry<'a, P: Ord, T: Values, C: alloc::raw::Extent = capacity::Count> {
    Scheduled(Slot<'a, T, indexed::Occupied<'a, P, C>, phase::Scheduled>),
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
        raw::clear(self, convert);
    }
}

impl<P: Ord, T: Values, C: alloc::raw::Extent> Drop for Min<P, T, C> {
    fn drop(&mut self) {
        raw::destroy(self);
    }
}

impl<'a, T: Values, H, F: Phase<T>> Slot<'a, T, H, F> {
    pub fn index(&self) -> usize {
        self.field.index()
    }

    pub fn get(&self) -> (&T::Common, &F::Item) {
        self.field.get()
    }

    pub fn get_mut(&mut self) -> (&mut T::Common, &mut F::Item) {
        self.field.get_mut()
    }

    pub fn change<G: Phase<T> + Retained<H>>(self, value: G::Item) -> (Slot<'a, T, H, G>, F::Item) {
        let (field, old) = self.field.change::<G>(value);
        (
            Slot {
                order: self.order,
                field,
            },
            old,
        )
    }
}

impl<'a, P: Ord, T: Values, C: alloc::raw::Extent, F: Phase<T>>
    Slot<'a, T, indexed::Occupied<'a, P, C>, F>
{
    pub fn replace(self, priority: P) -> (Self, P) {
        let (order, old) = self.order.replace(priority);
        (
            Self {
                order,
                field: self.field,
            },
            old,
        )
    }

    pub fn retire(self, value: T::Idle) -> (Vacant<'a, P, T, phase::Idle, C>, F::Item, P) {
        let (order, priority) = self.order.remove();
        let (field, old) = self.field.change::<phase::Idle>(value);
        (Slot { order, field }, old, priority)
    }
}

impl<'a, P: Ord, T: Values, C: alloc::raw::Extent, F: Phase<T>>
    Slot<'a, T, indexed::Vacant<'a, P, C>, F>
{
    pub fn schedule(
        self,
        priority: P,
        value: T::Control,
    ) -> (
        Slot<'a, T, indexed::Occupied<'a, P, C>, phase::Control>,
        F::Item,
    ) {
        let order = self.order.insert(priority);
        let (field, old) = self.field.change::<phase::Control>(value);
        (Slot { order, field }, old)
    }
}

pub mod raw;

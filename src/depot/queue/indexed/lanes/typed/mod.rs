//! Indexed FIFO lanes whose resident phase is retained by each occupied loan.

use std::marker;

pub(in crate::depot) use reserved::Reserved;
pub(in crate::depot) use running::Running;
pub(in crate::depot) use sealed::Sealed;

use crate::depot::{
    array::Index,
    capacity,
    map::direct,
    queue::indexed::lanes::{self, entry},
};

mod raw;
mod reserved;
mod running;
mod sealed;

/// An owned value, or a borrowed view of its one resident field.
#[derive(Debug)]
pub enum Value<C, D, I> {
    Control(C),
    Data(D),
    Idle(I),
}

/// Only active phases can belong to a lane.
#[derive(Debug)]
pub enum Active<C, D> {
    Control(C),
    Data(D),
}

#[derive(Debug)]
pub struct Control;
#[derive(Debug)]
pub struct Data;
#[derive(Debug)]
pub struct Idle;

/// A resident field type established by its exact table entry.
pub trait Phase<C, D, I>: Sealed {
    type Item;

    /// Wraps an owned field in this phase.
    fn value(value: Self::Item) -> Value<C, D, I>;
}

impl<C, D, I> Phase<C, D, I> for Control {
    type Item = C;

    fn value(value: C) -> Value<C, D, I> {
        Value::Control(value)
    }
}

impl<C, D, I> Phase<C, D, I> for Data {
    type Item = D;

    fn value(value: D) -> Value<C, D, I> {
        Value::Data(value)
    }
}

impl<C, D, I> Phase<C, D, I> for Idle {
    type Item = I;

    fn value(value: I) -> Value<C, D, I> {
        Value::Idle(value)
    }
}

/// One field and its exact entry authority, classified once at admission.
pub struct Slot<'a, K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I>, const N: usize> {
    slot: entry::Occupied<'a, K, raw::Stored<C, D, I>, N, raw::Tag>,
    phase: marker::PhantomData<fn() -> P>,
}

type ControlSlot<'a, K, C, D, I, const N: usize> = Slot<'a, K, C, D, I, Control, N>;
type DataSlot<'a, K, C, D, I, const N: usize> = Slot<'a, K, C, D, I, Data, N>;

/// An active control or data loan from its original table.
pub enum Front<'a, K: crate::depot::raw::Key, C, D, I, const N: usize> {
    Control(ControlSlot<'a, K, C, D, I, N>),
    Data(DataSlot<'a, K, C, D, I, N>),
}

/// An occupied entry classified by its current resident phase.
pub enum Entry<'a, K: crate::depot::raw::Key, C, D, I, const N: usize> {
    Control(ControlSlot<'a, K, C, D, I, N>),
    Data(DataSlot<'a, K, C, D, I, N>),
    Idle(Slot<'a, K, C, D, I, Idle, N>),
}

/// The exact key and original FIFO position of a consumed field.
#[must_use = "an unrestored withdrawal leaves its value removed"]
pub struct Withdrawn<'a, K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I>, const N: usize> {
    slot: entry::Withdrawn<'a, K, raw::Stored<C, D, I>, N, raw::Tag>,
    phase: marker::PhantomData<fn() -> P>,
}

/// One admitted empty table entry.
#[must_use = "a vacancy retains its exclusive schedule borrow"]
pub struct Vacant<'a, K: crate::depot::raw::Key, C, D, I, const N: usize> {
    slot: entry::Vacancy<'a, K, raw::Stored<C, D, I>, N, raw::Tag>,
}

/// An exact vacant entry that admits one unlinked successor phase.
pub trait Vacancy<'a, K: crate::depot::raw::Key, C, D, I, const N: usize>: Reserved {
    fn insert<P: Phase<C, D, I>>(self, value: P::Item) -> Slot<'a, K, C, D, I, P, N>;
}

/// Exact-key storage with one resident phase tag and exclusive FIFO membership.
pub struct Table<K: crate::depot::raw::Key, C, D, I, const N: usize> {
    values: lanes::Table<K, raw::Stored<C, D, I>, N, raw::Tag>,
}

impl<K: crate::depot::raw::Key, C, D, I, const N: usize> Table<K, C, D, I, N> {
    /// Bytes reserved for each exact key, value, and its lane links.
    pub const NODE_BYTES: usize = lanes::Table::<K, raw::Stored<C, D, I>, N, raw::Tag>::NODE_BYTES;

    pub fn try_new(capacity: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            values: lanes::Table::try_new(capacity)?,
        })
    }

    pub const fn capacity(&self) -> usize {
        self.values.capacity()
    }

    pub const fn len(&self) -> usize {
        self.values.len()
    }

    pub const fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    pub fn get(&self, key: K) -> Option<Value<&C, &D, &I>> {
        Some(raw::view(self.values.values.get(key)?))
    }

    pub fn entry_at(&self, index: usize) -> Option<(K, Value<&C, &D, &I>)> {
        let (key, value) = self.values.values.entry_at(index)?;
        Some((key, raw::view(value)))
    }

    pub fn key_at(&self, index: usize) -> Option<K> {
        self.values.key_at(index)
    }

    pub fn lane(&self, key: K) -> Option<Option<Index<N>>> {
        self.values.lane(key)
    }

    pub fn try_vacant(&mut self, key: K) -> Result<Vacant<'_, K, C, D, I, N>, direct::Error<K>> {
        Ok(Vacant {
            slot: self.values.try_vacant(key)?,
        })
    }

    pub fn vacant(&mut self, key: K) -> Option<Vacant<'_, K, C, D, I, N>> {
        Some(Vacant {
            slot: self.values.vacant(key)?,
        })
    }

    pub fn entry(&mut self, key: K) -> Option<Entry<'_, K, C, D, I, N>> {
        Some(raw::classify(self.values.entry(key)?))
    }

    pub fn back(&self, lane: Index<N>) -> Option<K> {
        self.values.back(lane)
    }

    pub fn is_vacant(&self, lane: Index<N>) -> bool {
        self.values.is_vacant(lane)
    }

    pub fn move_all(&mut self, source: Index<N>, target: Index<N>) -> usize {
        self.values.move_all(source, target)
    }

    pub fn clear(&mut self) {
        self.values.clear();
    }
}

impl<'a, K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I>, const N: usize>
    Slot<'a, K, C, D, I, P, N>
{
    pub fn key(&self) -> K {
        self.slot.key()
    }

    pub fn lane(&self) -> Option<Index<N>> {
        self.slot.lane()
    }

    pub fn remove(self) -> P::Item {
        self.take().1
    }
}

impl<K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I> + Running, const N: usize>
    Slot<'_, K, C, D, I, P, N>
{
    pub fn move_to_back(self, target: Index<N>) {
        self.slot.move_to_back(target);
    }

    pub fn unlink(self) {
        self.slot.unlink();
    }
}

impl<'a, K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I>, const N: usize>
    Withdrawn<'a, K, C, D, I, P, N>
{
    /// Restores the same phase at its original FIFO position.
    pub fn restore(self, value: P::Item) -> Slot<'a, K, C, D, I, P, N> {
        Slot {
            slot: self.slot.restore_with(raw::Stored::new(P::value(value))),
            phase: marker::PhantomData,
        }
    }
}

impl<'a, K: crate::depot::raw::Key, C, D, I, const N: usize> Vacancy<'a, K, C, D, I, N>
    for Vacant<'a, K, C, D, I, N>
{
    fn insert<P: Phase<C, D, I>>(self, value: P::Item) -> Slot<'a, K, C, D, I, P, N> {
        Slot {
            slot: self.slot.insert_with(raw::Stored::new(P::value(value))),
            phase: marker::PhantomData,
        }
    }
}

impl<'a, K: crate::depot::raw::Key, C, D, I, O: Phase<C, D, I>, const N: usize>
    Vacancy<'a, K, C, D, I, N> for Withdrawn<'a, K, C, D, I, O, N>
{
    fn insert<P: Phase<C, D, I>>(self, value: P::Item) -> Slot<'a, K, C, D, I, P, N> {
        Slot {
            slot: self.slot.insert_with(raw::Stored::new(P::value(value))),
            phase: marker::PhantomData,
        }
    }
}

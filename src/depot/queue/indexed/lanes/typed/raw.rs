use std::{marker, mem::ManuallyDrop, ptr};

use crate::depot::{
    array::Index,
    queue::indexed::lanes::{
        self, entry,
        typed::{Entry, Front, Phase, Slot, Table, Value, Withdrawn},
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Tag {
    Control,
    Data,
    Idle,
}

#[repr(C)]
pub(super) union Stored<C, D, I> {
    control: ManuallyDrop<C>,
    data: ManuallyDrop<D>,
    idle: ManuallyDrop<I>,
}

impl<C, D, I> Stored<C, D, I> {
    pub(super) fn new(value: Value<C, D, I>) -> lanes::raw::Owned<Self, Tag> {
        let (value, tag) = match value {
            Value::Control(value) => (
                Self {
                    control: ManuallyDrop::new(value),
                },
                Tag::Control,
            ),
            Value::Data(value) => (
                Self {
                    data: ManuallyDrop::new(value),
                },
                Tag::Data,
            ),
            Value::Idle(value) => (
                Self {
                    idle: ManuallyDrop::new(value),
                },
                Tag::Idle,
            ),
        };
        // SAFETY: the owner transfers the initialized union field and its tag together.
        unsafe { lanes::raw::Owned::from_parts(value, tag) }
    }
}

// SAFETY: this state owns the corresponding sole initialized union field.
unsafe impl<C, D, I> lanes::raw::State<Stored<C, D, I>> for Tag {
    unsafe fn clear(self, value: &mut ManuallyDrop<Stored<C, D, I>>) {
        unsafe {
            match self {
                Tag::Control => ManuallyDrop::drop(&mut value.control),
                Tag::Data => ManuallyDrop::drop(&mut value.data),
                Tag::Idle => ManuallyDrop::drop(&mut value.idle),
            }
        }
    }
}

pub(super) fn view<C, D, I, const N: usize>(
    node: &lanes::Node<Stored<C, D, I>, N, Tag>,
) -> Value<&C, &D, &I> {
    // SAFETY: the node owns the exact state-payload pair. Only the admitted
    // field is projected; neither the union nor its tag can escape.
    unsafe {
        match node.state {
            Tag::Control => Value::Control(&node.value.control),
            Tag::Data => Value::Data(&node.value.data),
            Tag::Idle => Value::Idle(&node.value.idle),
        }
    }
}

impl<'a, K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I>, const N: usize>
    Slot<'a, K, C, D, I, P, N>
{
    pub fn get(&self) -> &P::Item {
        // SAFETY: the sealed phase identifies this exact entry's initialized
        // field. The repr(C) union places every field at offset zero.
        unsafe { &*ptr::from_ref(self.slot.get()).cast::<P::Item>() }
    }

    pub fn get_mut(&mut self) -> &mut P::Item {
        // SAFETY: the exclusive occupied loan retains the admitted phase and
        // prevents replacement or retirement for the returned borrow.
        unsafe { &mut *ptr::from_mut(self.slot.get_mut()).cast::<P::Item>() }
    }

    pub fn take(self) -> (Withdrawn<'a, K, C, D, I, P, N>, P::Item) {
        let (slot, value) = self.slot.take_owned();
        let (value, _) = value.into_parts();
        // SAFETY: the retained phase identifies the sole initialized field.
        // Taking the exact occupied slot detached its links and transferred its
        // value here. Move that field once and suppress its former owner's Drop.
        let field = unsafe { ptr::from_ref(&value).cast::<P::Item>().read() };
        (
            Withdrawn {
                slot,
                phase: marker::PhantomData,
            },
            field,
        )
    }
}

pub(super) fn classify<K: crate::depot::raw::Key, C, D, I, const N: usize>(
    slot: entry::Occupied<'_, K, Stored<C, D, I>, N, Tag>,
) -> Entry<'_, K, C, D, I, N> {
    match slot.state() {
        Tag::Control => Entry::Control(Slot {
            slot,
            phase: marker::PhantomData,
        }),
        Tag::Data => Entry::Data(Slot {
            slot,
            phase: marker::PhantomData,
        }),
        Tag::Idle => Entry::Idle(Slot {
            slot,
            phase: marker::PhantomData,
        }),
    }
}

impl<K: crate::depot::raw::Key, C, D, I, const N: usize> Table<K, C, D, I, N> {
    /// The table owns the queue proof: only active loans can link, and successors enter unlinked.
    /// Restoration preserves the original phase and membership, including for Idle.
    /// Membership therefore proves this front is active; unlinked residents may also be active.
    pub fn front(&mut self, lane: Index<N>) -> Option<Front<'_, K, C, D, I, N>> {
        let slot = self.values.front(lane)?;
        Some(if slot.state() == Tag::Control {
            Front::Control(Slot {
                slot,
                phase: marker::PhantomData,
            })
        } else {
            Front::Data(Slot {
                slot,
                phase: marker::PhantomData,
            })
        })
    }
}

use std::{marker, mem, ptr};

use crate::depot::{
    arena,
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

/// Unit keys place the sole initialized field of these repr(C) unions at offset zero.
/// Nesting the existing union adds no stored tag.
#[repr(C)]
pub(super) struct Stored<C, D, I> {
    value: arena::raw::StagedValue<(), C, arena::raw::StagedValue<(), D, I>>,
}

impl<C, D, I> Stored<C, D, I> {
    pub(super) fn new(value: Value<C, D, I>) -> lanes::raw::Owned<Self, Tag> {
        let (value, tag) = match value {
            Value::Control(value) => (arena::raw::StagedValue::queued((), value), Tag::Control),
            Value::Data(value) => (
                arena::raw::StagedValue::parked((), arena::raw::StagedValue::queued((), value)),
                Tag::Data,
            ),
            Value::Idle(value) => (
                arena::raw::StagedValue::parked((), arena::raw::StagedValue::parked((), value)),
                Tag::Idle,
            ),
        };
        // SAFETY: this match initialized exactly the sole field selected by tag.
        // The returned owner transfers the pair together into its resident node.
        unsafe { lanes::raw::Owned::from_parts(Self { value }, tag) }
    }
}

// SAFETY: this state owns the corresponding sole initialized union field.
unsafe impl<C, D, I> lanes::raw::State<Stored<C, D, I>> for Tag {
    unsafe fn clear(self, value: &mut mem::ManuallyDrop<Stored<C, D, I>>) {
        // SAFETY: exactly the tagged field remains initialized. The nested unit
        // keys own no resources; each active field is dropped exactly once.
        unsafe {
            match self {
                Tag::Control => value.value.clear_queued(),
                Tag::Data => value.value.parked_mut().clear_queued(),
                Tag::Idle => value.value.parked_mut().clear_parked(),
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
            Tag::Control => Value::Control(node.value.value.queued_ref()),
            Tag::Data => Value::Data(node.value.value.parked_ref().queued_ref()),
            Tag::Idle => Value::Idle(node.value.value.parked_ref().parked_ref()),
        }
    }
}

impl<'a, K: crate::depot::raw::Key, C, D, I, P: Phase<C, D, I>, const N: usize>
    Slot<'a, K, C, D, I, P, N>
{
    pub fn get(&self) -> &P::Item {
        // SAFETY: the sealed phase identifies this exact entry's initialized
        // field. The repr(C) unit-key unions place that field at offset zero.
        unsafe { &*ptr::addr_of!(self.slot.get().value).cast::<P::Item>() }
    }

    pub fn get_mut(&mut self) -> &mut P::Item {
        // SAFETY: the exclusive occupied loan retains the admitted phase and
        // prevents replacement or retirement for the returned borrow.
        unsafe { &mut *ptr::addr_of_mut!(self.slot.get_mut().value).cast::<P::Item>() }
    }

    pub fn take(self) -> (Withdrawn<'a, K, C, D, I, P, N>, P::Item) {
        let (slot, value) = self.slot.take_owned();
        let (value, _) = value.into_parts();
        // SAFETY: the retained phase identifies the sole initialized field.
        // Taking the exact occupied slot detached its links and transferred its
        // value here. Move that field once and suppress its former owner's Drop.
        let field = unsafe { ptr::read(ptr::addr_of!(value.value).cast::<P::Item>()) };
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

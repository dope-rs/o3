use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        hold, segments,
        segments::{Chain, Handle, Key, Plural, handles},
    },
    depot::arena::linked::slots,
};

pub(in crate::buffer::segments) enum Remaining {
    Single,
    Multi,
}

pub(in crate::buffer::segments) struct Multi<'a, T, H: Handle<T>> {
    pub(in crate::buffer::segments) sequence: &'a mut Chain<T, H>,
    pub(in crate::buffer::segments) index: slots::Index,
}

/// The detached front and metadata owned by one exclusive sequence.
/// Its stable extents remain part of that sequence's admitted totals until finalization.
struct Removed<'a, T, H: Handle<T>> {
    owner: slots::Live<segments::Item<T>, &'a slots::Store<segments::Item<T>>>,
    handle: &'a H,
    key: &'a mut Key,
    index: slots::Index,
}

// SAFETY: Multi retains its initialized descriptor and exact nonempty node partition.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::Linked<'a> for &'a mut Multi<'_, T, H> {
    type Item = segments::Item<T>;
    type Metadata = handles::Descriptor;

    fn parts(
        self,
    ) -> (
        &'a slots::Store<Self::Item>,
        &'a slots::Store<Self::Metadata>,
        slots::Index,
    ) {
        let handle = &self.sequence.handle;
        (
            handles::nodes(handle),
            handles::descriptors(handle),
            self.index,
        )
    }

    fn head(metadata: &Self::Metadata) -> slots::Index {
        metadata.links.head_raw()
    }
}

// SAFETY: this Multi exclusively borrows the original sequence and both stores are disjoint.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::LinkedMut<'a> for &'a mut Multi<'_, T, H> {}

// SAFETY: detached-front metadata retains the descriptor and remaining live node partition.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::Linked<'a> for &'a mut Removed<'_, T, H> {
    type Item = segments::Item<T>;
    type Metadata = handles::Descriptor;

    fn parts(
        self,
    ) -> (
        &'a slots::Store<Self::Item>,
        &'a slots::Store<Self::Metadata>,
        slots::Index,
    ) {
        (
            handles::nodes(self.handle),
            handles::descriptors(self.handle),
            self.index,
        )
    }

    fn head(metadata: &Self::Metadata) -> slots::Index {
        metadata.links.head_raw()
    }
}

// SAFETY: Removed retains the exclusive sequence-key loan through finalization.
unsafe impl<'a, T: 'a, H: Handle<T>> buffer::raw::LinkedMut<'a> for &'a mut Removed<'_, T, H> {}

impl<'a, T: buffer::raw::Segment, H: Handle<T>> Multi<'a, T, H> {
    pub(in crate::buffer::segments) fn front(
        &mut self,
    ) -> (&mut handles::Descriptor, &mut segments::Item<T>) {
        buffer::raw::LinkedMut::front_mut(self)
    }

    pub(in crate::buffer::segments::chain::edit) fn consume(
        mut self,
        amount: NonZero<usize>,
    ) -> Option<NonZero<usize>> {
        let (desc, owner) = self.front();
        if let Some(tail) = segments::Tail::positive(owner, amount) {
            tail.commit();
            desc.len -= amount.get();
            return None;
        }
        let owner = self.take();
        NonZero::new(amount.get() - owner.len().get())
    }

    pub(in crate::buffer::segments) fn take(self) -> segments::Item<T> {
        self.take_front().0
    }

    pub(in crate::buffer::segments) fn take_front(self) -> (segments::Item<T>, Remaining) {
        self.detach().take()
    }

    pub(in crate::buffer::segments) fn discard(self) -> (hold::Charge, Remaining) {
        self.detach().finish()
    }

    fn detach(self) -> Removed<'a, T, H> {
        let nodes = handles::nodes(&self.sequence.sequence.handle);
        let descs = handles::descriptors(&self.sequence.sequence.handle);
        // SAFETY: this head exclusively borrows the sequence owning this descriptor.
        let desc = unsafe { &mut *descs.value_ptr(self.index) };
        let head = desc.links.head_raw();
        let owner = unsafe { crate::depot::raw::SlotOwner::detach(&mut desc.links, nodes, head) };
        Removed {
            owner,
            handle: &self.sequence.sequence.handle,
            key: &mut self.sequence.sequence.key,
            index: self.index,
        }
    }
}

impl<T: buffer::raw::Segment, H: Handle<T>> Removed<'_, T, H> {
    fn take(mut self) -> (segments::Item<T>, Remaining) {
        let rest = self.finish().1;
        let owner = self.owner.take().0;
        (owner, rest)
    }

    fn finish(&mut self) -> (hold::Charge, Remaining) {
        let len = self.owner.get().len().get();
        let resident = buffer::Residency::resident(self.owner.get());
        let charge = hold::Charge::from_owner(len, resident);
        let descs = handles::descriptors(self.handle);
        let desc = buffer::raw::LinkedMut::metadata_mut(&mut *self);
        let count = Plural(desc.count);
        let rest = if count.raw() == 2 {
            let entry = unsafe { descs.detached(self.index) };
            *self.key = Key::single(entry.get().links.head_raw());
            Remaining::Single
        } else {
            desc.count = count.tail();
            desc.len -= len;
            desc.resident_bytes -= resident;
            Remaining::Multi
        };
        (charge, rest)
    }
}

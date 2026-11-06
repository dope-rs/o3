use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        hold, segments,
        segments::{Chain, Handle, Key, Nonempty, Plural, Shape, Tail, handles},
    },
    depot::arena::linked::slots,
    mem::rc,
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

impl<'a, T: buffer::raw::Segment, H: Handle<T>> Multi<'a, T, H> {
    pub(in crate::buffer::segments) fn front(
        &mut self,
    ) -> (&mut handles::Descriptor, &mut segments::Item<T>) {
        let handle = &self.sequence.handle;
        let nodes = handles::nodes(handle);
        let descs = handles::descriptors(handle);
        // SAFETY: Multi exclusively retains this live descriptor and its head
        // in distinct stores belonging to the same sequence.
        unsafe {
            let desc = &mut *descs.value_ptr(self.index);
            let owner = &mut *nodes.value_ptr(desc.links.head_raw());
            (desc, owner)
        }
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

    #[inline(always)]
    pub(in crate::buffer::segments) fn take_front(self) -> (segments::Item<T>, Remaining) {
        let mut removed = self.detach();
        let rest = removed.finish().1;
        let owner = removed.owner.take().0;
        (owner, rest)
    }

    pub(in crate::buffer::segments) fn discard(self) -> (hold::Charge, Remaining) {
        self.detach().finish()
    }

    fn detach(self) -> Removed<'a, T, H> {
        let nodes = handles::nodes(&self.sequence.handle);
        let descs = handles::descriptors(&self.sequence.handle);
        // SAFETY: this head exclusively borrows the sequence owning this descriptor.
        let desc = unsafe { &mut *descs.value_ptr(self.index) };
        let head = desc.links.head_raw();
        let owner = unsafe { crate::depot::raw::SlotOwner::detach(&mut desc.links, nodes, head) };
        Removed {
            owner,
            handle: &self.sequence.handle,
            key: &mut self.sequence.key,
            index: self.index,
        }
    }
}

impl<T: buffer::raw::Segment, H: Handle<T>> Removed<'_, T, H> {
    #[inline(always)]
    fn finish(&mut self) -> (hold::Charge, Remaining) {
        let len = self.owner.get().len().get();
        let resident = buffer::Residency::resident(self.owner.get());
        let charge = hold::Charge::from_owner(len, resident);
        let descs = handles::descriptors(self.handle);
        // SAFETY: Removed retains the exclusive sequence-key loan and this live descriptor.
        let desc = unsafe { &mut *descs.value_ptr(self.index) };
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

impl<T: buffer::raw::Segment> Nonempty<T> {
    pub fn try_pop(self, limit: usize) -> Result<(T, Shape<T>), (rc::Error, Self)> {
        match self {
            Self::One(mut owner) => {
                let Some(tail) = Tail::new(&mut owner.value, limit) else {
                    return Ok((owner.into_inner(), Shape::Empty));
                };
                match tail.try_commit() {
                    Ok(head) => Ok((head, Shape::One(owner))),
                    Err(error) => Err((error, Self::One(owner))),
                }
            }
            Self::Many(owners) => {
                let (head, rest) = owners
                    .try_pop(limit)
                    .map_err(|(error, owners)| (error, Self::Many(owners)))?;
                Ok((head, rest.into_shape()))
            }
        }
    }
}

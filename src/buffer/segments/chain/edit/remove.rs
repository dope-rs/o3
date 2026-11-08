use crate::{
    buffer,
    buffer::{
        hold, segments,
        segments::{Chain, Handle, Key, Nonempty, Plural, Shape, handles},
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

type Removed<'a, T> = (
    slots::Live<segments::Item<T>, &'a slots::Store<segments::Item<T>>>,
    hold::Charge,
    Remaining,
);

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

    #[inline(always)]
    pub(in crate::buffer::segments) fn take_front(self) -> (segments::Item<T>, Remaining) {
        let (owner, _, rest) = self.remove();
        (owner.take().0, rest)
    }

    pub(in crate::buffer::segments) fn discard(self) -> (hold::Charge, Remaining) {
        let (_, charge, rest) = self.remove();
        (charge, rest)
    }

    #[inline(always)]
    fn remove(self) -> Removed<'a, T> {
        let nodes = handles::nodes(&self.sequence.handle);
        let descs = handles::descriptors(&self.sequence.handle);
        // SAFETY: this head exclusively borrows the sequence owning this descriptor.
        let desc = unsafe { &mut *descs.value_ptr(self.index) };
        let head = desc.links.head_raw();
        let owner = unsafe { crate::depot::raw::SlotOwner::detach(&mut desc.links, nodes, head) };
        let len = owner.get().len().get();
        let resident = buffer::Residency::resident(owner.get());
        let charge = hold::Charge::from_owner(len, resident);
        let count = Plural(desc.count);
        let rest = if count.raw() == 2 {
            let entry = unsafe { descs.detached(self.index) };
            self.sequence.key = Key::single(entry.get().links.head_raw());
            Remaining::Single
        } else {
            desc.count = count.tail();
            desc.len -= len;
            desc.resident_bytes -= resident;
            Remaining::Multi
        };
        (owner, charge, rest)
    }
}

impl<T: buffer::raw::Segment> Nonempty<T> {
    pub fn try_pop(self, limit: usize) -> Result<(T, Shape<T>), (rc::Error, Self)> {
        match self {
            Self::One(mut owner) => {
                let Some(tail) = owner.tail(limit) else {
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

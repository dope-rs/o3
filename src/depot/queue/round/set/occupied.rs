use std::{marker, mem};

use crate::depot::{
    arena::slab::{self, entries, key},
    capacity,
    queue::{links, round::set},
};

/// One occupied resident retaining its exact order and exclusive slot loan.
pub struct Occupied<
    'a,
    T,
    Tag = (),
    const MAX: u32 = { u32::MAX },
    const ROOT: bool = true,
    B = (),
    P = (),
> {
    pub(in crate::depot::queue::round) entry:
        slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<false>, 1>, Tag, ROOT>,
    pub(in crate::depot::queue::round) order: &'a set::Order<B>,
    pub(in crate::depot::queue::round) index: links::Index,
    pub(in crate::depot::queue::round) phase: marker::PhantomData<P>,
}

impl<'a, T, Tag, const MAX: u32, const ROOT: bool, B, P> Occupied<'a, T, Tag, MAX, ROOT, B, P> {
    pub(in crate::depot::queue::round) fn new(
        entry: slab::Occupied<entries::Busy<'a, T, MAX, entries::Exclusive<false>, 1>, Tag, ROOT>,
        order: &'a set::Order<B>,
    ) -> Self {
        // SAFETY: the joint owner allocated this resident and hook in the same Count.
        let index = unsafe { links::Index::new_usize(entry.key().index() as usize) };
        Self {
            entry,
            order,
            index,
            phase: marker::PhantomData,
        }
    }

    pub(in crate::depot::queue::round) fn phase<Q>(self) -> Occupied<'a, T, Tag, MAX, ROOT, B, Q> {
        Occupied {
            entry: self.entry,
            order: self.order,
            index: self.index,
            phase: marker::PhantomData,
        }
    }

    pub fn get(&self) -> &T {
        self.entry.get()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.entry.get_mut()
    }

    pub fn into_mut(self) -> &'a mut T {
        self.entry.into_mut()
    }

    pub fn state(self) -> set::State<'a, T, Tag, MAX, ROOT, B> {
        use crate::depot::raw::Hooks as _;

        // SAFETY: this resident retains its original owner and in-bounds hook.
        let hook = unsafe { self.order.hook_raw(self.index) };
        if self.index.is_linked(hook) {
            set::State::Ready(self.phase())
        } else {
            set::State::Idle(self.phase())
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Occupied<'static, ()>>() == 4 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Option<Occupied<'static, ()>>>() == mem::size_of::<Occupied<'static, ()>>()
    );
};

impl<T, Tag, const MAX: u32, const ROOT: bool, P> Occupied<'_, T, Tag, MAX, ROOT, (), P> {
    pub fn key(&self) -> key::Handle<Tag, MAX> {
        self.entry.key()
    }
}

impl<'h, T, Tag, const MAX: u32, const ROOT: bool, P>
    Occupied<'_, T, Tag, MAX, ROOT, capacity::Domain<'h>, P>
{
    pub fn key(&self) -> capacity::Key<'h, key::Handle<Tag, MAX>> {
        // SAFETY: the joint set allocated values and this order in the same domain.
        capacity::Key::new(unsafe { crate::depot::raw::Proof::new(self.entry.key()) })
    }
}

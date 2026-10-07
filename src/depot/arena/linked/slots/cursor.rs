use std::mem;

use crate::depot::{
    self,
    arena::{
        linked,
        linked::{head, slots},
    },
};

/// Insertion cursor with only predecessor and current positions.
pub struct Cursor<'a, T, Tag> {
    store: &'a slots::Store<T>,
    state: &'a mut head::List<Tag>,
    previous: slots::Index<Tag>,
    current: slots::Index<Tag>,
}

impl<'a, T, Tag> Cursor<'a, T, Tag> {
    /// # Safety
    /// The state owns one acyclic live partition in this exact store.
    pub(in crate::depot::arena::linked) const unsafe fn new(
        store: &'a slots::Store<T>,
        state: &'a mut head::List<Tag>,
    ) -> Self {
        Self {
            store,
            current: state.head,
            state,
            previous: slots::Index::NONE,
        }
    }

    pub fn current(&self) -> Option<&T> {
        let current = self.current;
        if current == slots::Index::NONE {
            return None;
        }
        Some(unsafe { self.store.value(current) })
    }

    pub fn advance(&mut self) {
        let current = self.current;
        if current.get().is_none() {
            return;
        }
        self.previous = current;
        self.current = unsafe { self.store.next(current) };
    }

    /// Publishes the initialized detached claim into its proved original node set.
    pub fn insert(proof: Member<'_, 'a, Self, T, Tag>) {
        let (cursor, claim) = proof.into_inner();
        let inserted = claim.index();
        unsafe { cursor.store.set_next(inserted, cursor.current) };
        if cursor.previous.get().is_none() {
            cursor.state.head = inserted;
        } else {
            unsafe { cursor.store.set_next(cursor.previous, inserted) };
        }
        cursor.previous = inserted;
        unsafe { claim.publish() };
    }
}

const _: () = assert!(mem::size_of::<Cursor<'static, u64, ()>>() == 3 * mem::size_of::<usize>());

type Member<'s, 'a, S, T, Tag> =
    depot::raw::Proof<(&'s mut S, slots::Live<T, &'a slots::Store<T>, Tag>), linked::Same>;

use std::mem;

use crate::depot::arena::linked::{head, slots};

/// The unchanged list front tied to its store and owner borrows.
#[must_use]
pub struct Entry<'a, 'b, T, Tag> {
    store: &'a slots::Store<T>,
    state: &'b mut head::List<Tag>,
}

impl<'a, 'b, T, Tag> Entry<'a, 'b, T, Tag> {
    /// # Safety
    /// The state owns a nonempty acyclic live partition in this exact store.
    pub(in crate::depot::arena::linked) const unsafe fn new(
        store: &'a slots::Store<T>,
        state: &'b mut head::List<Tag>,
    ) -> Self {
        Self { store, state }
    }

    pub fn get(&self) -> &T {
        unsafe { self.store.value(self.state.head) }
    }

    pub fn pop(self) -> T {
        self.take().0
    }

    pub fn take(self) -> (T, slots::Vacant<T, &'a slots::Store<T>, Tag>) {
        let index = self.state.head;
        self.state.head = unsafe { self.store.next(index) };
        unsafe { self.store.detached(index) }.take()
    }
}

const _: () =
    assert!(mem::size_of::<Entry<'static, 'static, u64, ()>>() == 2 * mem::size_of::<usize>());

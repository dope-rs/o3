pub(crate) use pop::EndsPop;

use crate::depot::{
    self,
    arena::linked::{
        fifo::ends::{self, Cell, Exclusive},
        slots,
    },
};
mod pop;

unsafe impl<Tag> crate::depot::raw::SlotOwner<Tag> for ends::Exclusive<slots::Index<Tag>> {
    fn links(&self) -> &Self {
        self
    }

    unsafe fn detach<'a, T>(
        &mut self,
        store: &'a slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, &'a slots::Store<T>, Tag> {
        let index = unsafe { self.detach_store(store, index) };
        unsafe { store.detached(index) }
    }
}

unsafe impl<L: depot::raw::Link> EndsPop<L> for &mut Exclusive<L> {
    unsafe fn pop<T, R>(
        self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R {
        self.0.pop(front, input, pop)
    }
}

unsafe impl<L: depot::raw::Link> EndsPop<L> for &Cell<L> {
    unsafe fn pop<T, R>(
        self,
        front: L::Item,
        input: T,
        pop: impl FnOnce(L::Item, T) -> (L, R),
    ) -> R {
        let mut state = self.0.get();
        let result = state.pop(front, input, pop);
        self.0.set(state);
        result
    }
}

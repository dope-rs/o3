pub(crate) use owner::SlotOwner;

use crate::depot::arena::linked::{
    fifo::ends,
    slots,
    slots::{Index, State},
};
mod owner;

unsafe impl<Tag> crate::depot::raw::Link for Index<Tag> {
    type Item = Self;

    const NONE: Self = Self::NONE;

    fn item(self) -> Option<Self> {
        (self != Self::NONE).then_some(self)
    }

    unsafe fn some(item: Self) -> Self {
        item
    }
}

unsafe impl<Tag> crate::depot::raw::SlotOwner<Tag> for State<Tag> {
    fn links(&self) -> &ends::Exclusive<slots::Index<Tag>> {
        &self.links
    }

    unsafe fn detach<'a, T>(
        &mut self,
        store: &'a slots::Store<T>,
        index: slots::Index<Tag>,
    ) -> slots::Live<T, &'a slots::Store<T>, Tag> {
        unsafe { State::detach(self, store, index) }
    }
}

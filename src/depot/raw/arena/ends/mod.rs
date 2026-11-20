pub(crate) use pop::EndsPop;

mod pop;

unsafe impl<Tag> crate::depot::raw::Link for crate::depot::arena::linked::slots::Index<Tag> {
    type Item = Self;

    const NONE: Self = Self::NONE;

    fn item(self) -> Option<Self> {
        (self != Self::NONE).then_some(self)
    }

    unsafe fn some(item: Self) -> Self {
        item
    }
}

use std::marker;

use crate::depot::queue::tagged;

pub struct LaneUndo<E>(pub(in crate::depot) marker::PhantomData<E>);

impl<E: crate::depot::raw::TagEntry> crate::mem::Return for LaneUndo<E>
where
    E::Store: crate::depot::raw::LaneStore,
{
    type Item = E;

    fn return_item(self, entry: E) {
        let entry: tagged::Linked<E> = unsafe { tagged::Linked::new(entry) };
        entry.remove();
    }
}

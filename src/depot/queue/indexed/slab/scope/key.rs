use std::marker;

use crate::depot::queue::links;

/// A live slot in one generative, insertion-only mutation scope.
pub struct Key<'id, Tag> {
    pub(in crate::depot) index: links::Index,
    pub(in crate::depot) brand: marker::PhantomData<fn(&'id mut Tag) -> &'id mut Tag>,
}

impl<Tag> Copy for Key<'_, Tag> {}
impl<Tag> Clone for Key<'_, Tag> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<Tag> PartialEq for Key<'_, Tag> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<Tag> Eq for Key<'_, Tag> {}

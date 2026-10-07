use crate::{
    buffer,
    buffer::{pool, pool::owners, segments, storage},
    mem::rc,
};

#[repr(transparent)]
pub struct NonEmpty<'view, 'source, S: ?Sized>(&'view buffer::View<'source, S>);

impl<'view, 'source, S: ?Sized> NonEmpty<'view, 'source, S> {
    pub(in crate::buffer) fn new(view: &'view buffer::View<'source, S>) -> Option<Self> {
        if view.is_empty() {
            return None;
        }
        Some(Self(view))
    }

    /// # Safety
    /// The source-bound view exposes at least one byte.
    pub(in crate::buffer) unsafe fn assume(view: &'view buffer::View<'source, S>) -> Self {
        Self(view)
    }

    pub(in crate::buffer) fn as_slice(&self) -> &'source [u8] {
        self.0.as_slice()
    }

    pub(in crate::buffer) fn source(&self) -> &'source S {
        self.0.source()
    }

    pub(in crate::buffer) fn view(&self) -> &'view buffer::View<'source, S> {
        self.0
    }
}

impl<'view, 'source, S: ?Sized> From<&'view segments::Item<buffer::View<'source, S>>>
    for NonEmpty<'view, 'source, S>
{
    fn from(view: &'view segments::Item<buffer::View<'source, S>>) -> Self {
        Self(view.as_ref())
    }
}

impl<L> NonEmpty<'_, '_, storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'_>>>> {
    pub(in crate::buffer) fn try_own(self) -> Result<storage::Shared, rc::Error> {
        Ok(self.0.try_clone()?.into_owner())
    }
}

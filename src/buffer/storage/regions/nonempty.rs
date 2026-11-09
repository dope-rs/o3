use crate::buffer::{self, segments};

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

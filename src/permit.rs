use std::mem;

/// Receives a resource when its owning [`Permit`] is dropped.
pub trait Return {
    type Item;

    fn return_item(&self, item: Self::Item);
}

/// Returns its item to the sink when dropped.
#[must_use]
pub struct Permit<R: Return> {
    item: mem::ManuallyDrop<R::Item>,
    sink: R,
}

impl<R: Return> Permit<R> {
    pub fn new(sink: R, item: R::Item) -> Self {
        Self {
            item: mem::ManuallyDrop::new(item),
            sink,
        }
    }
}

impl<R: Return> Drop for Permit<R> {
    fn drop(&mut self) {
        // SAFETY: new initializes item, this is its only extraction site, and
        // Drop runs at most once for a value.
        let item = unsafe { mem::ManuallyDrop::take(&mut self.item) };
        self.sink.return_item(item);
    }
}

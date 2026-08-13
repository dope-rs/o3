use std::{mem, ptr};

/// Receives a resource when its owning [`Lease`] is dropped.
pub trait Return {
    type Item;

    fn return_item(&self, item: Self::Item);
}

/// Returns its item to the sink when dropped.
#[must_use]
pub struct Lease<R: Return> {
    item: mem::ManuallyDrop<R::Item>,
    sink: R,
}

impl<R: Return> Lease<R> {
    pub fn new(sink: R, item: R::Item) -> Self {
        Self {
            item: mem::ManuallyDrop::new(item),
            sink,
        }
    }

    pub fn sink(&self) -> &R {
        &self.sink
    }

    pub fn item(&self) -> &R::Item {
        &self.item
    }

    /// Disarms this lease and returns its sink and item without returning the
    /// item to the sink.
    pub fn into_parts(self) -> (R, R::Item) {
        let mut this = mem::ManuallyDrop::new(self);
        // SAFETY: ManuallyDrop suppresses Lease::drop. Each initialized field
        // is moved exactly once, and neither field is subsequently accessed.
        unsafe {
            let sink = ptr::read(&this.sink);
            let item = mem::ManuallyDrop::take(&mut this.item);
            (sink, item)
        }
    }
}

impl<R: Return> Drop for Lease<R> {
    fn drop(&mut self) {
        // SAFETY: new initializes item, this is its only extraction site, and
        // Drop runs at most once for a value.
        let item = unsafe { mem::ManuallyDrop::take(&mut self.item) };
        self.sink.return_item(item);
    }
}

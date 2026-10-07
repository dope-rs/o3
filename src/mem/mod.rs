use std::{marker, mem, ptr};

pub use rejected::Rejected;

pub mod credit;
pub mod lease;
pub mod quota;
pub mod raw;
pub mod rc;
mod rejected;
pub mod slice;
pub mod slot;

/// Receives an item when its owning [`Lease`] is dropped.
pub trait Return {
    type Item;

    fn return_item(self, item: Self::Item);
}

/// Returns its item to the sink when dropped.
///
/// The default sealed mode permits observation and return, but not mutation or
/// disarming. Those operations require [`lease::Open`].
#[must_use = "dropping the lease returns its item"]
pub struct Lease<R: Return, M = lease::Sealed> {
    sink: mem::ManuallyDrop<R>,
    item: mem::ManuallyDrop<R::Item>,
    mode: marker::PhantomData<M>,
}

impl<R: Return> Lease<R> {
    pub const fn new(sink: R, item: R::Item) -> Self {
        Self {
            sink: mem::ManuallyDrop::new(sink),
            item: mem::ManuallyDrop::new(item),
            mode: marker::PhantomData,
        }
    }
}

impl<R: Return> Lease<R, lease::Open> {
    pub const fn open(sink: R, item: R::Item) -> Self {
        Self {
            sink: mem::ManuallyDrop::new(sink),
            item: mem::ManuallyDrop::new(item),
            mode: marker::PhantomData,
        }
    }

    pub fn item_mut(&mut self) -> &mut R::Item {
        &mut self.item
    }

    /// Mutably borrows the return contract and its unchanged item together.
    pub fn parts_mut(&mut self) -> (&mut R, &mut R::Item) {
        (&mut self.sink, &mut self.item)
    }

    /// Disarms this lease and returns its sink and item.
    pub fn into_parts(self) -> (R, R::Item) {
        let mut this = mem::ManuallyDrop::new(self);

        // SAFETY: ManuallyDrop suppresses Lease::drop. Both initialized fields
        // are moved exactly once and are not accessed afterward.
        unsafe {
            let sink = mem::ManuallyDrop::take(&mut this.sink);
            let item = mem::ManuallyDrop::take(&mut this.item);
            (sink, item)
        }
    }
}

impl<R: Return, M> Lease<R, M> {
    pub fn sink(&self) -> &R {
        &self.sink
    }

    pub const fn item(&self) -> &R::Item {
        // SAFETY: ManuallyDrop<T> has the same layout as T and the field stays
        // initialized for the duration of this shared borrow.
        unsafe { &*ptr::from_ref(&self.item).cast::<R::Item>() }
    }
}

impl<R: Return, M> Drop for Lease<R, M> {
    fn drop(&mut self) {
        // SAFETY: Both fields are initialized by new, extracted only here, and
        // Drop runs at most once. The sink is consumed after the item is moved.
        unsafe {
            let sink = mem::ManuallyDrop::take(&mut self.sink);
            let item = mem::ManuallyDrop::take(&mut self.item);
            sink.return_item(item);
        }
    }
}

use std::{marker, mem};

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
    parts: mem::ManuallyDrop<(R, R::Item)>,
    mode: marker::PhantomData<M>,
}

impl<R: Return> Lease<R> {
    pub const fn new(sink: R, item: R::Item) -> Self {
        Self {
            parts: mem::ManuallyDrop::new((sink, item)),
            mode: marker::PhantomData,
        }
    }
}

impl<R: Return> Lease<R, lease::Open> {
    pub const fn open(sink: R, item: R::Item) -> Self {
        Self {
            parts: mem::ManuallyDrop::new((sink, item)),
            mode: marker::PhantomData,
        }
    }

    pub fn item_mut(&mut self) -> &mut R::Item {
        &mut self.parts.1
    }

    /// Mutably borrows the return contract and its unchanged item together.
    pub fn parts_mut(&mut self) -> (&mut R, &mut R::Item) {
        let (sink, item) = &mut *self.parts;
        (sink, item)
    }

    /// Disarms this lease and returns its sink and item.
    pub fn into_parts(self) -> (R, R::Item) {
        let mut this = mem::ManuallyDrop::new(self);

        // SAFETY: ManuallyDrop suppresses Lease::drop; the initialized pair
        // is moved exactly once and is not accessed afterward.
        unsafe { mem::ManuallyDrop::take(&mut this.parts) }
    }
}

impl<R: Return, M> Lease<R, M> {
    pub fn sink(&self) -> &R {
        &self.parts.0
    }

    pub fn item(&self) -> &R::Item {
        &self.parts.1
    }
}

impl<R: Return, M> Drop for Lease<R, M> {
    fn drop(&mut self) {
        // SAFETY: the initialized pair is transferred once before the sink
        // consumes its item; the ManuallyDrop field has no later destruction.
        let (sink, item) = unsafe { mem::ManuallyDrop::take(&mut self.parts) };
        sink.return_item(item);
    }
}

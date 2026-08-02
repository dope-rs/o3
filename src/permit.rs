//! Linear permits that return a resource when dropped.

/// Receives a resource when its owning [`ReturnPermit`] is dropped.
pub trait ReturnTo {
    type Item;

    fn return_item(&self, item: Self::Item);
}

/// The unique right to either retain a resource or transfer it elsewhere.
///
/// Dropping the permit returns its item to `sink`. Extracting the item requires
/// consuming the permit, so safe code cannot return the item while a borrow of
/// the permit's owner is live.
#[must_use]
pub struct ReturnPermit<R: ReturnTo> {
    item: Option<R::Item>,
    sink: R,
}

impl<R: ReturnTo> ReturnPermit<R> {
    pub fn new(sink: R, item: R::Item) -> Self {
        Self {
            item: Some(item),
            sink,
        }
    }

    /// Transfers the resource out of this permit.
    ///
    /// A live permit always contains its item: construction is private to
    /// [`Self::new`], and the only operation that removes it consumes `self`.
    pub fn into_item(mut self) -> R::Item {
        self.item.take().expect("o3: live return permit")
    }
}

impl<R: ReturnTo> Drop for ReturnPermit<R> {
    fn drop(&mut self) {
        if let Some(item) = self.item.take() {
            self.sink.return_item(item);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{ReturnPermit, ReturnTo};

    struct Sink<'a>(&'a Cell<Option<u8>>);

    impl ReturnTo for Sink<'_> {
        type Item = u8;

        fn return_item(&self, item: u8) {
            assert_eq!(self.0.replace(Some(item)), None);
        }
    }

    #[test]
    fn drop_returns_the_item() {
        let returned = Cell::new(None);
        {
            let _permit = ReturnPermit::new(Sink(&returned), 7);
        }
        assert_eq!(returned.get(), Some(7));
    }

    #[test]
    fn consuming_the_permit_transfers_without_returning() {
        let returned = Cell::new(None);
        let item = ReturnPermit::new(Sink(&returned), 7).into_item();
        assert_eq!(item, 7);
        assert_eq!(returned.get(), None);
    }
}

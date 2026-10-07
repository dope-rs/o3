use crate::depot::index::{
    bitmap,
    bitmap::entry::{Removed, Row},
};

/// One set bit kept unchanged by an exclusive borrow of its source.
pub struct Occupied;

impl<'a, W: bitmap::Width, S: crate::depot::raw::Bitmap<W>> Row<&'a mut S, S::Plane, Occupied, W> {
    pub fn remove(self) -> Row<&'a mut S, S::Plane, Removed, W> {
        let row: Row<_, _, (), W> = self.with_phase();
        row.update::<false>(row.plane());
        row.with_phase()
    }
}

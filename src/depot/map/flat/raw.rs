use crate::depot::map::{self, flat::Table};

/// The retained table and admission context own one unused logical entry.
/// Its nonempty directory has spare capacity through the consuming query.
pub struct Available;

impl crate::depot::raw::Sealed for Available {}
impl crate::depot::Predicate for Available {}

/// This exact table retains an initialized entry with the retained hash, satisfying
/// every invocation of the retained matcher throughout the query. Its context
/// preserves membership until the occupied entry and original context are returned.
pub struct Member;

impl crate::depot::raw::Sealed for Member {}
impl crate::depot::Predicate for Member {}

unsafe impl<V> map::raw::Mutation for &mut Table<V> {
    unsafe fn table_mut(&mut self) -> &mut map::raw::Table<V> {
        &mut self.table
    }

    unsafe fn shift(&mut self, hole: &map::raw::Vacant, next: &map::raw::Occupied, hash: u64) {
        unsafe { self.table.shift(hole, next, hash) };
    }
}

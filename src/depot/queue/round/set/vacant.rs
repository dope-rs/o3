use crate::depot::{
    arena::{slab, slab::entries},
    queue::round::set,
};

/// One reserved physical slot that cannot yet appear in the schedule.
pub struct Vacant<'a, T, Tag = (), const MAX: u32 = { u32::MAX }, const ROOT: bool = true, B = ()> {
    pub(in crate::depot::queue::round) entry:
        slab::Vacant<entries::Pending<'a, T, MAX, entries::Exclusive<false>, 1>, Tag, ROOT>,
    pub(in crate::depot::queue::round) order: &'a set::Order<B>,
}

impl<'a, T, Tag, const MAX: u32, const ROOT: bool, B> Vacant<'a, T, Tag, MAX, ROOT, B> {
    pub fn insert(self, value: T) -> set::Occupied<'a, T, Tag, MAX, ROOT, B, set::Idle> {
        let entry = self.entry.stage(value).commit();
        set::Occupied::new(entry, self.order)
    }
}

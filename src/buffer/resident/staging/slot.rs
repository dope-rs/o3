use crate::buffer::{pool, pool::owners};

pub(in crate::buffer) struct Slot<'a, 'd> {
    pub(in crate::buffer::resident::staging) cursor: &'a mut pool::Span<owners::Resident<'d>>,
    pub(in crate::buffer::resident::staging) head: &'a mut u32,
}

impl<'a, 'd> Slot<'a, 'd> {
    pub(in crate::buffer) fn into_parts(
        self,
    ) -> (&'a mut pool::Span<owners::Resident<'d>>, &'a mut u32) {
        (self.cursor, self.head)
    }
}

const _: () = assert!(size_of::<Slot<'static, 'static>>() == 2 * size_of::<usize>());

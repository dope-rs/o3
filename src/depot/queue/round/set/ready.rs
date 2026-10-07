use crate::depot::queue::round::set;

/// An occupied resident proved present in its owner's schedule.
#[derive(Debug)]
pub struct Ready;

impl<'a, T, Tag, const MAX: u32, const ROOT: bool, B>
    set::Occupied<'a, T, Tag, MAX, ROOT, B, Ready>
{
    pub fn remove(self) -> set::Occupied<'a, T, Tag, MAX, ROOT, B, set::Idle> {
        let entry = self;
        // SAFETY: the retained exclusive resident is linked in this exact order.
        unsafe { entry.order.ring().unlink_key(entry.order, entry.index) };
        entry.phase()
    }

    pub fn requeue(self) -> Self {
        let ring = self.order.ring();
        // SAFETY: this live member proves that its exact order remains nonempty.
        let first = unsafe { ring.first_live() };
        // SAFETY: the exclusive resident retains this member and its current front.
        unsafe { ring.move_back::<_, false>(self.order, self.index, first) };
        self
    }
}

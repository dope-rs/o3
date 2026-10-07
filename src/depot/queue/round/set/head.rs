use crate::depot::queue::round::set;

/// The exclusively borrowed resident at this owner's current fair turn.
#[derive(Debug)]
pub struct Head;

impl<'a, T, Tag, const MAX: u32, B> set::Occupied<'a, T, Tag, MAX, true, B, Head> {
    pub fn advance(self) -> &'a mut T {
        let entry = self;
        // SAFETY: the joint owner's head cannot change during this exclusive loan.
        unsafe { entry.order.ring().next_live(entry.order, entry.index) };
        entry.into_mut()
    }

    pub fn remove(self) -> set::Occupied<'a, T, Tag, MAX, true, B, set::Idle> {
        let entry = self;
        // SAFETY: this retained resident is the live front of this exact order.
        unsafe { entry.order.ring().pop_front(entry.order, entry.index) };
        entry.phase()
    }
}

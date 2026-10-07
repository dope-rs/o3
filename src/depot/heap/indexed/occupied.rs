use crate::{
    alloc,
    depot::{capacity, heap},
};

/// Retains the exact heap and the authority for this entry phase.
#[repr(transparent)]
pub struct Occupied<'a, P: Ord, C: alloc::raw::Extent = capacity::Count> {
    pub(in crate::depot::heap) entry: heap::Entry<&'a mut indexed::Min<P, C>>,
}

use std::num;

use heap::indexed;
impl<'a, P: Ord, C: alloc::raw::Extent> indexed::Occupied<'a, P, C> {
    pub fn index(&self) -> usize {
        self.entry.value().identity.get()
    }

    pub fn position(&self) -> num::NonZero<u32> {
        self.entry.value().identity.position()
    }

    pub fn priority(&self) -> &P {
        &self.entry.value().priority
    }

    pub fn replace(self, value: P) -> (Self, P) {
        let (entry, old) = heap::raw::Hole::replace(self.entry.prepare(value));
        (Self { entry }, old.priority)
    }

    pub fn set(self, value: P) {
        heap::raw::Hole::update(self.entry.prepare(value));
    }

    pub fn retire(self) {
        heap::raw::Hole::retire(self.entry);
    }

    pub fn take(self) -> P {
        heap::raw::Hole::take(self.entry).priority
    }

    pub fn remove(self) -> (indexed::Vacant<'a, P, C>, P) {
        let (entry, value) = self.entry.remove_slot();
        (indexed::Vacant { entry }, value)
    }
}

const _: () = {
    assert!(
        std::mem::size_of::<indexed::Occupied<'static, u64>>() == 2 * std::mem::size_of::<usize>()
    );
    assert!(
        std::mem::size_of::<Option<indexed::Occupied<'static, u64>>>()
            == std::mem::size_of::<indexed::Occupied<'static, u64>>()
    );
};

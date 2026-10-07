use std::cell::Cell;

use crate::depot::{
    self,
    queue::round::{self, phase},
};

/// One initialized current front retained until a callback-free consuming transition.
pub struct Head<'a, T: Copy, const G: usize, const N: usize = 0>(
    pub(super) round::Location<&'a round::Lanes<Cell<Option<T>>, G, N>, phase::Head>,
);

impl<T: Copy, const G: usize, const N: usize> Head<'_, T, G, N> {
    pub fn index(&self) -> usize {
        self.0.index()
    }

    pub fn get(&self) -> T {
        let value = self.0.get().get();
        // SAFETY: this closed owner's live Head proves Some; the local copy's
        // initialization cannot change during its retained loan.
        unsafe { *crate::mem::raw::initialized(&value) }
    }

    pub fn pop(self) -> T {
        let value = self.get();
        self.0.pop().set(None);
        value
    }

    pub fn rotate(self) -> Self {
        Self(self.0.rotate())
    }

    pub(super) fn consume(source: depot::raw::Proof<Self, phase::Active>) -> T {
        let head = source.into_inner();
        let value = head.get();
        // SAFETY: projection preserves the same owner's exact active-front membership.
        round::Location::take_active(unsafe { depot::raw::Proof::new(head.0) }).set(None);
        value
    }
}

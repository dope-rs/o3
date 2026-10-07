use std::ops;

use crate::{base::cell::region, buffer::storage::provided::fixed};

/// The sole terminal owner loan for this arena generation.
#[repr(transparent)]
pub struct Loan<'d, T> {
    pub(in crate::buffer::storage::provided) target: &'d mut T,
    pub(in crate::buffer::storage::provided) brand: region::Proof<region::Identity<'d>>,
}

impl<'d, T> Loan<'d, T> {
    pub fn finish<P>(self, proof: P) -> fixed::End<'d, T, P> {
        fixed::End { loan: self, proof }
    }
}

impl<T> ops::Deref for Loan<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.target
    }
}

impl<T> ops::DerefMut for Loan<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.target
    }
}

const _: () = {
    assert!(size_of::<Loan<'_, ()>>() == size_of::<usize>());
    assert!(size_of::<Option<Loan<'_, ()>>>() == size_of::<usize>());
    assert!(align_of::<Loan<'_, ()>>() == align_of::<usize>());
};

use crate::buffer::storage::provided::fixed;

/// Completed terminal authority retaining its original loan and exact proof.
pub struct End<'d, T, P> {
    pub(in crate::buffer::storage::provided::fixed) loan: fixed::Loan<'d, T>,
    pub(in crate::buffer::storage::provided::fixed) proof: P,
}

impl<T, P> End<'_, T, P> {
    pub(in crate::buffer::storage::provided) fn release<U>(&mut self, item: U)
    where
        P: fixed::Release<T, U>,
    {
        self.proof.release(&mut self.loan, item);
    }
}

const _: () = {
    assert!(size_of::<End<'_, (), ()>>() == size_of::<usize>());
    assert!(size_of::<Option<End<'_, (), ()>>>() == size_of::<usize>());
    assert!(align_of::<End<'_, (), ()>>() == align_of::<usize>());
};

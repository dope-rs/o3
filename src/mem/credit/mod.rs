use std::num;

pub use error::Error;

mod error;
pub mod fair;

/// A thread-confined budget whose acquisitions return on drop.
pub struct Ledger {
    limit: usize,
    remaining: crate::mem::raw::Account,
}

/// The unforgeable return authority issued with one positive acquisition.
pub struct Return<'a> {
    source: &'a Ledger,
}

impl Ledger {
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            remaining: crate::mem::raw::Account::new(limit),
        }
    }

    pub fn try_acquire(
        &self,
        amount: num::NonZero<usize>,
    ) -> Result<crate::mem::Lease<Return<'_>>, Error> {
        self.remaining.try_acquire(amount, 0)?;
        Ok(crate::mem::Lease::new(Return { source: self }, amount))
    }

    pub const fn limit(&self) -> usize {
        self.limit
    }

    pub fn available(&self) -> usize {
        self.remaining.get()
    }
}

impl crate::mem::Return for Return<'_> {
    type Item = num::NonZero<usize>;

    fn return_item(self, amount: Self::Item) {
        self.source.remaining.refund(amount.get());
    }
}

const _: () = {
    assert!(size_of::<Ledger>() == size_of::<usize>() * 2);
    assert!(size_of::<crate::mem::Lease<Return<'static>>>() == size_of::<usize>() * 2);
    assert!(
        size_of::<Option<crate::mem::Lease<Return<'static>>>>()
            == size_of::<crate::mem::Lease<Return<'static>>>()
    );
};

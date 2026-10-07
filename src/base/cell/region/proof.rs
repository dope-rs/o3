use crate::base::cell::{
    region::{self, Brand, Identity},
    values,
};

/// Copyable identity proof carried by capabilities derived from a region token.
#[derive(Clone, Copy, Debug)]
#[repr(transparent)]
pub struct Proof<B: Brand> {
    pub(super) proof: values::Proof<B>,
}

impl<B: Brand> Proof<B> {
    /// Binds newly owned state to this exact region identity.
    pub const fn bind<T>(self, value: T) -> region::Value<B, T> {
        region::Value {
            value: self.proof.bind(value),
        }
    }
}

const _: () = {
    assert!(size_of::<Proof<Identity<'static>>>() == 0);
    assert!(size_of::<Proof<Identity<'static, 1>>>() == 0);
};

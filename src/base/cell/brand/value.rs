use crate::base::cell::{
    brand::{Brand, Identity, Token},
    values,
};

/// A value accessible only with the matching application [`Token`].
#[repr(transparent)]
pub struct Value<B: Brand, T> {
    pub(in crate::base::cell) value: values::Value<B, T>,
}

const _: () = {
    assert!(size_of::<Token<Identity<'static>>>() == 0);
    assert!(size_of::<Value<Identity<'static>, u64>>() == size_of::<u64>());
    assert!(align_of::<Value<Identity<'static>, u64>>() == align_of::<u64>());
};

use std::marker::PhantomData;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ThreadBound(PhantomData<*mut ()>);

impl ThreadBound {
    pub const NEW: Self = Self(PhantomData);
}

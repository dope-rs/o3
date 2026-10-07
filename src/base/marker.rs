use std::marker;

/// Public zero-sized proof of constructing-thread confinement.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug)]
pub struct ThreadBound {
    pub marker: marker::PhantomData<*mut ()>,
}

impl ThreadBound {
    pub const NEW: Self = Self {
        marker: marker::PhantomData,
    };
}

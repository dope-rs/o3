use std::{marker, mem};

/// Public zero-sized proof of constructing-thread confinement.
#[repr(transparent)]
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ThreadBound {
    pub marker: marker::PhantomData<*mut ()>,
}

impl ThreadBound {
    pub const NEW: Self = Self {
        marker: marker::PhantomData,
    };
}

const _: () = {
    assert!(mem::size_of::<ThreadBound>() == 0);
    assert!(mem::align_of::<ThreadBound>() == 1);
};

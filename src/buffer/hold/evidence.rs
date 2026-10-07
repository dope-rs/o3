//! Compile-time representation of retained-owner evidence.

use crate::buffer::hold;

/// Selects the exact stored representation of retained-owner evidence.
pub trait Mode {
    type Projection: Copy + Default;
}

/// Erases retained-owner evidence and its storage from a monomorphization.
#[derive(Clone, Copy, Default)]
pub struct Off;

impl Mode for Off {
    type Projection = Self;
}

/// Preserves the exact physical-owner charge for evidence delivery.
pub enum On {}

impl Mode for On {
    type Projection = hold::Charge;
}

const _: () = {
    assert!(core::mem::size_of::<<Off as Mode>::Projection>() == 0);
    assert!(
        core::mem::size_of::<<On as Mode>::Projection>() == core::mem::size_of::<hold::Charge>()
    );
};

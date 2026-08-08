mod seal;

pub(crate) use seal::Seal;

#[doc(hidden)]
pub struct Initialized;

#[doc(hidden)]
pub struct Uninitialized;

#[doc(hidden)]
pub trait State: Seal {
    const ZEROED: bool;
}

impl Seal for Initialized {}

impl State for Initialized {
    const ZEROED: bool = true;
}

impl Seal for Uninitialized {}

impl State for Uninitialized {
    const ZEROED: bool = false;
}

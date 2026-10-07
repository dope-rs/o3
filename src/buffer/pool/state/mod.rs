mod seal;
pub(in crate::buffer::pool::state) use seal::Seal;

#[doc(hidden)]
pub struct Initialized;

#[doc(hidden)]
pub struct Uninit;

#[doc(hidden)]
pub trait State: Seal {
    const ZEROED: bool;
}

impl Seal for Initialized {}

impl State for Initialized {
    const ZEROED: bool = true;
}

impl Seal for Uninit {}

impl State for Uninit {
    const ZEROED: bool = false;
}

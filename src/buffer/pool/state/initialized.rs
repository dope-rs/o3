use crate::buffer::pool::state::{State, sealed::Sealed};

#[doc(hidden)]
pub struct Initialized;

impl Sealed for Initialized {}

impl State for Initialized {
    const ZEROED: bool = true;
}

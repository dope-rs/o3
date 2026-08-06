use crate::buffer::pool::state::{State, sealed::Sealed};

#[doc(hidden)]
pub struct Uninitialized;

impl Sealed for Uninitialized {}

impl State for Uninitialized {
    const ZEROED: bool = false;
}

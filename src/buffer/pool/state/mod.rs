mod initialized;
mod sealed;
mod uninitialized;

pub use initialized::Initialized;
use sealed::Sealed;
pub use uninitialized::Uninitialized;

#[doc(hidden)]
pub trait State: Sealed {
    const ZEROED: bool;
}

//! Bounded incremental receive storage with a precharged warm path.

use std::num;

use backing::Backing;
pub use buf::Buf;
pub(in crate::buffer) use buf::Reserve;
pub use layout::Layout;
pub use plan::Plan;
pub use pool::Pool;
pub(in crate::buffer) use slot::Slot;

use crate::buffer::pool::LayoutError;

mod backing;
mod layout;
mod plan;
pub mod reserve;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    ZeroWarm,
    WarmExceedsLimit {
        warm: num::NonZero<usize>,
        limit: usize,
    },
    LimitTooLarge {
        limit: num::NonZero<usize>,
    },
    ZeroBuffers,
    SlotsOverflow {
        buffers: num::NonZero<usize>,
        shared: usize,
    },
    SlotsTooLarge {
        buffers: num::NonZero<usize>,
        shared: usize,
        cause: num::TryFromIntError,
    },
    Pool(LayoutError),
    ResidentOverflow {
        buffers: num::NonZero<usize>,

        plan: Plan,
        shared: usize,
        reserve: usize,
    },
}

mod buf;
mod pool;
mod slot;

#[derive(Clone, Copy)]
pub(in crate::buffer::resident) enum Mode {
    Local,
    Accounted,
}

pub(in crate::buffer) use header::Header;
mod header;

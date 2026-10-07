//! Indexed payloads whose hash directory and FIFO planes share one owner.

use core::{Core, Hooks};

pub use error::Error;
pub use occupied::Occupied;
pub use record::Record;
pub use slot::Slot;
pub use state::State;
pub use table::Table;
pub use vacancy::Vacancy;

mod error;

mod core;
mod occupied;
mod record;
mod slot;
mod state;
mod table;
mod vacancy;

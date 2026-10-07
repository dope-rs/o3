use provided::account;
use storage::range;

use crate::buffer::{storage, storage::provided};

/// Exact reason a provided owner could not enter one resident domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    Range(range::Error),
    Account(account::Cause),
}

const _: () = assert!(size_of::<Error>() == 4 * size_of::<usize>());

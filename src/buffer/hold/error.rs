use resident::copy;
use storage::{provided::account, range};

use crate::buffer::{resident, storage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Range(range::Error),
    Resident(resident::AllocError),
    Copy(copy::Error),
    Account(account::Cause),
}

impl From<range::Error> for Error {
    fn from(error: range::Error) -> Self {
        Self::Range(error)
    }
}

impl From<resident::AllocError> for Error {
    fn from(error: resident::AllocError) -> Self {
        Self::Resident(error)
    }
}

impl From<copy::Error> for Error {
    fn from(error: copy::Error) -> Self {
        Self::Copy(error)
    }
}

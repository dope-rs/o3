use std::{convert, mem, num::NonZero};

use crate::depot::{
    arena::slab::{Refusal, key},
    capacity,
};

/// The exact key and physical observation that rejected an exclusive lookup.
#[derive(Debug)]
pub enum Error<Tag = (), const MAX: u32 = { u32::MAX }, B = convert::Infallible, R = ()> {
    Index {
        key: key::Handle<Tag, MAX>,
        capacity: capacity::Count,
    },
    Free {
        key: key::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
    },
    Reserved {
        key: key::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
    },
    Retired {
        key: key::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
        retired: R,
    },
    Generation {
        key: key::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
    },
    Busy {
        key: key::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
        borrow: B,
    },
}

impl<Tag, const MAX: u32, B, R> Error<Tag, MAX, B, R> {
    pub(in crate::depot::arena::slab) fn refused(
        key: key::Handle<Tag, MAX>,
        actual: key::Generation<MAX>,
        refusal: Refusal<R>,
    ) -> Self {
        match refusal {
            Refusal::Free => Self::Free { key, actual },
            Refusal::Reserved => Self::Reserved { key, actual },
            Refusal::Retired(retired) => Self::Retired {
                key,
                actual,
                retired,
            },
            Refusal::Stale => Self::Generation { key, actual },
        }
    }

    /// Borrows the rejected identity and observation as diagnostic metadata.
    /// The scalar identity carries no branded handle or reconstruction authority.
    pub fn observation(&self) -> (u32, NonZero<u32>, key::Observation<MAX, &B, &R>) {
        use key::Observation;

        let (key, observed) = match self {
            Self::Index { key, capacity } => (
                key,
                Observation::Index {
                    capacity: *capacity,
                },
            ),
            Self::Free { key, actual } => (key, Observation::Free { actual: *actual }),
            Self::Reserved { key, actual } => (key, Observation::Reserved { actual: *actual }),
            Self::Retired {
                key,
                actual,
                retired,
            } => (
                key,
                Observation::Retired {
                    actual: *actual,
                    retired,
                },
            ),
            Self::Generation { key, actual } => (key, Observation::Generation { actual: *actual }),
            Self::Busy {
                key,
                actual,
                borrow,
            } => (
                key,
                Observation::Busy {
                    actual: *actual,
                    borrow,
                },
            ),
        };
        (key.index(), key.generation().0, observed)
    }

    pub fn into_parts(self) -> (key::Handle<Tag, MAX>, key::Observation<MAX, B, R>) {
        use key::Observation;

        match self {
            Self::Index { key, capacity } => (key, Observation::Index { capacity }),
            Self::Free { key, actual } => (key, Observation::Free { actual }),
            Self::Reserved { key, actual } => (key, Observation::Reserved { actual }),
            Self::Retired {
                key,
                actual,
                retired,
            } => (key, Observation::Retired { actual, retired }),
            Self::Generation { key, actual } => (key, Observation::Generation { actual }),
            Self::Busy {
                key,
                actual,
                borrow,
            } => (key, Observation::Busy { actual, borrow }),
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Error>() == 2 * mem::size_of::<u64>());
    assert!(mem::size_of::<Option<Error>>() == mem::size_of::<Error>());
};

use crate::mem::{credit, rc};

/// Exact state that prevented one provided owner from entering a domain.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    Foreign,
    Exhausted { count: u32, limit: u32 },
    Budget(credit::Error),
    Retain(rc::Error),
}

const _: () = assert!(size_of::<Cause>() == 3 * size_of::<usize>());

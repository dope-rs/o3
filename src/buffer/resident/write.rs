use crate::buffer::write::consume;

/// The exact obstacle to extending an acquired resident suffix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Cause {
    Capacity(consume::Error),
    Shared(consume::Error),
    Rotation(consume::Error),
}

const _: () = {
    assert!(size_of::<Cause>() == 3 * size_of::<usize>());
};

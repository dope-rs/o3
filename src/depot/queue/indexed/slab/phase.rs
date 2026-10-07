use crate::depot::array::Index;

/// The reservation action performed when an admitted slot is abandoned.
pub trait State: Copy {
    const RELEASE: bool;
    const RETIRE: bool;
}

#[derive(Clone, Copy, Debug)]
pub struct Vacant;
#[derive(Clone, Copy, Debug)]
pub struct Occupied;
#[derive(Clone, Copy, Debug)]
pub(in crate::depot) struct Metadata;
#[derive(Clone, Copy, Debug)]
pub(in crate::depot) struct Detached;

#[derive(Clone, Copy, Debug)]
pub struct Target<const N: usize>(pub(in crate::depot) Index<N>);
#[derive(Clone, Copy, Debug)]
pub struct Restore<const N: usize>(pub(in crate::depot) Index<N>);

impl State for Vacant {
    const RELEASE: bool = true;
    const RETIRE: bool = false;
}
impl State for Occupied {
    const RELEASE: bool = false;
    const RETIRE: bool = false;
}
impl State for Metadata {
    const RELEASE: bool = false;
    const RETIRE: bool = false;
}
impl State for Detached {
    const RELEASE: bool = false;
    const RETIRE: bool = false;
}
impl<const N: usize> State for Target<N> {
    const RELEASE: bool = true;
    const RETIRE: bool = false;
}
impl<const N: usize> State for Restore<N> {
    const RELEASE: bool = false;
    const RETIRE: bool = true;
}

pub(in crate::depot) trait Initialized: State {}
impl Initialized for Metadata {}
impl Initialized for Occupied {}

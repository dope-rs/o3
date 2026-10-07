use crate::depot::queue::round::{Conflict, Idle, Location};

/// Failed admission retaining observed membership and the requested lane.
#[derive(Debug)]
pub enum Error<O> {
    Lane(Conflict<O>),
    Vacant(Location<O, Idle<true>>),
}

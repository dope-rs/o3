use std::{cell, marker};

pub(in crate::depot) use moves::StagedMove;
pub(in crate::depot) use node::StagedNode;
pub(in crate::depot) use occupied::StagedPhase;
pub(in crate::depot) use retired::Retired;

use crate::depot::queue::links;

mod moves;
mod node;
mod occupied;
mod retired;

#[repr(C)]
pub(in crate::depot) struct StagedState<I> {
    pub(in crate::depot) head: cell::Cell<Option<links::Index>>,
    pub(in crate::depot) tail: cell::Cell<Option<links::Index>>,
    pub(in crate::depot) parked: cell::Cell<Option<links::Index>>,
    pub(in crate::depot) occupied: cell::Cell<u32>,
    index: marker::PhantomData<fn(I) -> I>,
}

impl<I> StagedState<I> {
    pub(in crate::depot) const fn new() -> Self {
        Self {
            head: cell::Cell::new(None),
            tail: cell::Cell::new(None),
            parked: cell::Cell::new(None),
            occupied: cell::Cell::new(0),
            index: marker::PhantomData,
        }
    }
}

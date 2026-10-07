use crate::{
    alloc,
    depot::heap::{
        indexed,
        indexed::resident::{Sealed, phase},
    },
};

pub trait Retained<H>: Sealed {}

impl<'a, P: Ord, C: alloc::raw::Extent> Retained<indexed::Occupied<'a, P, C>> for phase::Scheduled {}
impl<'a, P: Ord, C: alloc::raw::Extent> Retained<indexed::Vacant<'a, P, C>> for phase::Idle {}
impl<'a, P: Ord, C: alloc::raw::Extent> Retained<indexed::Vacant<'a, P, C>> for phase::Control {}
impl<'a, P: Ord, C: alloc::raw::Extent> Retained<indexed::Vacant<'a, P, C>> for phase::Data {}
impl<'a, P: Ord, C: alloc::raw::Extent> Retained<indexed::Vacant<'a, P, C>> for phase::Active {}

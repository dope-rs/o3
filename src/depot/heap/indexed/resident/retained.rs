use crate::depot::heap::{
    Direct,
    indexed::resident::{Sealed, phase},
};

pub trait Retained<H>: Sealed {}

impl Retained<usize> for phase::Scheduled {}
impl Retained<Direct> for phase::Idle {}
impl Retained<Direct> for phase::Control {}
impl Retained<Direct> for phase::Data {}
impl Retained<Direct> for phase::Active {}

use std::{cell, marker};

use o3::depot::heap::indexed::resident;

use crate::common::{self, depot::model::drops};

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
#[repr(align(128))]
pub struct Aligned(pub u8);

pub const CAPACITY_257_BYTES: usize = 6_688;

pub struct Phases<'a>(marker::PhantomData<&'a ()>);

impl<'a> resident::Values for Phases<'a> {
    type Common = drops::Payload<'a>;
    type Scheduled = Self::Common;
    type Idle = Self::Common;
    type Control = Self::Common;
    type Data = Self::Common;
    type Active = Self::Common;
}

pub fn payload<'a>(id: usize, drops: &'a [cell::Cell<Option<usize>>]) -> drops::Payload<'a> {
    drops::Payload {
        id: id as u64,
        drops: common::Counted(&drops[id]),
    }
}

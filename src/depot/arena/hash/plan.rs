use std::mem;

use crate::{
    base::num::bounded,
    depot::{capacity, map},
};

#[derive(Clone, Copy)]
pub struct Plan {
    pub(super) table: map::raw::Shape<bounded::U32<0, { u32::MAX - 1 }>>,
}

impl Plan {
    pub fn new(cap: usize) -> Result<Self, map::Capacity> {
        Self::try_from(map::Plan::new(cap).map_err(map::Capacity::Domain)?)
    }

    pub const fn capacity(self) -> capacity::Count {
        self.table.capacity()
    }
}

impl TryFrom<map::Plan> for Plan {
    type Error = map::Capacity;

    fn try_from(plan: map::Plan) -> Result<Self, Self::Error> {
        let table = map::raw::Layout::try_plan(plan).map_err(|cause| map::Capacity::Layout {
            requested: plan.capacity().get(),
            cause,
        })?;
        Ok(Self {
            table: table.shape(),
        })
    }
}

const _: () = {
    assert!(mem::size_of::<Plan>() == mem::size_of::<u32>());
};

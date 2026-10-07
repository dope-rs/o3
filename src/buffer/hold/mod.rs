//! Allocation-owner retention projected onto accounting authorities.

use std::num::NonZero;

pub use charge::Charge;
pub use error::Error;

use crate::buffer::{self, resident, segments, storage};

mod charge;
mod error;
pub mod evidence;
pub mod limits;
pub mod ranges;
pub mod raw;
pub mod scope;

/// Exact retention operation selected by a policy.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Empty,
    Copy,
    Share,
}

impl Choice {
    pub(in crate::buffer::hold) fn try_retain<'a, 'd, R: raw::Retain<'d> + 'a>(
        self,
        plan: &R::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, Error> {
        match self {
            Self::Empty => Ok(resident::Shared::new()),
            Self::Copy => R::try_copy(plan, budget),
            Self::Share => R::try_retain(plan, budget),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    min: usize,
    factor: usize,
}

impl Policy {
    pub const fn new(min: usize, factor: usize) -> Self {
        Self { min, factor }
    }

    pub const fn compact() -> Self {
        Self::new(usize::MAX, 0)
    }

    /// Prefers every shareable owner. Pair this policy with a hard resident budget.
    pub const fn share() -> Self {
        Self::new(0, usize::MAX)
    }

    pub fn choose(self, logical: usize, resident: impl FnOnce() -> usize) -> Choice {
        let Some(logical) = NonZero::new(logical) else {
            return Choice::Empty;
        };
        if self.allows_share(logical, resident) {
            Choice::Share
        } else {
            Choice::Copy
        }
    }

    pub(in crate::buffer) fn allows_share(
        self,
        logical: NonZero<usize>,
        resident: impl FnOnce() -> usize,
    ) -> bool {
        let logical = logical.get();
        if logical < self.min || self.factor == 0 {
            return false;
        }
        let limit = logical.saturating_mul(self.factor);
        limit == usize::MAX || resident() <= limit
    }

    pub const fn try_limit(self, logical: usize) -> Result<limits::Bound, limits::Error> {
        if logical < self.min || self.factor == 0 {
            return Ok(limits::Bound::new(logical));
        }
        match logical.checked_mul(self.factor) {
            Some(resident) => Ok(limits::Bound::new(resident)),
            None => Err(limits::Error::Scale {
                logical,
                factor: self.factor,
            }),
        }
    }
}

const _: () = assert!(core::mem::size_of::<Choice>() == 1);
const _: () = assert!(core::mem::size_of::<Policy>() == 2 * core::mem::size_of::<usize>());

/// Exact component and operands that overflowed while combining charges.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Overflow {
    Logical { left: usize, right: usize },
    Resident { left: usize, right: usize },
    Owners { left: usize, right: usize },
}

const _: () = assert!(core::mem::size_of::<Overflow>() == 3 * core::mem::size_of::<usize>());

/// Copy authority for one retained-owner family.
#[derive(Debug)]
pub struct Retention<C: buffer::raw::Segment> {
    authority: C::Retention,
}

impl<C: buffer::raw::Segment> Default for Retention<C> {
    fn default() -> Self {
        Self {
            authority: C::Retention::default(),
        }
    }
}

impl<C: buffer::raw::Segment> Retention<C> {
    /// Copies bytes into this owner family, preserving its allocation authority.
    /// Empty input needs neither an authority nor an allocation.
    pub fn try_copy(&self, bytes: &[u8]) -> Result<C, CopyError> {
        let Some(source) = buffer::raw::Slices::try_new([bytes]).transpose() else {
            return Ok(C::EMPTY);
        };
        Ok(C::try_copy(source, &self.authority)?.into_inner())
    }

    pub(in crate::buffer) fn try_item(
        &self,
        source: buffer::raw::Slices<'_, 1>,
    ) -> Result<segments::Item<C>, CopyError> {
        C::try_copy(Ok(source), &self.authority)
    }
}

impl<'d> Retention<resident::Shared<'d>> {
    /// Binds once to an exact resident domain. Rebinding that same domain is inert.
    pub fn try_bind(&mut self, budget: resident::Budget<'d>) -> Result<(), resident::BindError> {
        budget.try_bind(&mut self.authority)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyError {
    Storage(storage::AllocError),
    Resident(resident::AllocError),
    Authority(resident::BindError),
}

const _: () = {
    assert!(size_of::<Retention<storage::Shared>>() == 0);
    assert!(size_of::<Retention<resident::Shared<'static>>>() == size_of::<usize>());
};

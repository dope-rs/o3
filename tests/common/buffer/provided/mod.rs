use std::{cell::Cell, collections, num::NonZero, pin::Pin};

pub use fixed::Fixed;
pub use input::Input;
use o3::{
    alloc,
    buffer::{bytes::bounded, resident, storage::provided},
};
use provided::view;

use crate::common::{
    Counted,
    alloc::system::{Activity, Tracker},
};

mod fixed;
mod input;
pub mod scope;

#[derive(Debug)]
pub struct Item<'a> {
    _drops: Counted<&'a Cell<Option<usize>>>,
    pub bytes: Vec<u8>,
}

impl<'a> Item<'a> {
    pub fn try_new(
        bytes: &[u8],
        drops: &'a Cell<Option<usize>>,
    ) -> Result<Self, collections::TryReserveError> {
        let mut owned = Vec::new();
        owned.try_reserve_exact(bytes.len())?;
        owned.extend_from_slice(bytes);
        Ok(Self {
            _drops: Counted(drops),
            bytes: owned,
        })
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        &mut self.bytes
    }

    pub fn try_scope(
        f: impl for<'d> FnOnce(
            provided::Unique<'d>,
            resident::Budget<'d>,
            &provided::Root<'d, Item<'_>>,
        ),
    ) -> Result<(), Error> {
        let drops = Cell::new(Some(0));
        let mut arena =
            provided::Arena::<Item>::try_with(NonZero::<u32>::MIN.saturating_add(1), || {
                Ok::<(), alloc::Error>(())
            })
            .map_err(Error::Allocation)?;

        let mut domain = resident::Domain::new();
        let (stats, resident, returned) =
            arena.scope(Pin::new(&mut domain), |token, arena, domain| {
                let budget = domain
                    .get_mut()
                    .try_budget(7, token.proof())
                    .map_err(Error::Budget)?;
                let buffer = Item::try_new(b"exactly", &drops).map_err(Error::Buffer)?;
                let slot = arena.vacant().ok_or(Error::SlotEmpty)?;
                let unique = slot.try_insert(buffer, 7, Item::bytes_mut);
                let unique = unique.map_err(|rejected| Error::Projection(rejected.discard()))?;
                let ((), stats) = Tracker::activity(|| f(unique, budget, &arena));
                Ok::<_, Error>((stats, budget.resident(), arena.pop_returned().is_some()))
            })?;

        let count = drops.get();
        if stats.allocations == 0
            && stats.deallocations == 0
            && resident == 0
            && returned
            && count == Some(1)
        {
            Ok(())
        } else {
            Err(Error::Scope {
                stats,
                resident,
                returned,
                drops: count,
            })
        }
    }
}

#[derive(Debug)]
pub enum Error {
    Allocation(alloc::Error),
    Budget(resident::Reuse),
    Buffer(collections::TryReserveError),
    SlotEmpty,
    Projection(view::LengthError),
    Bounded(bounded::Overflow),
    Scope {
        stats: Activity,
        resident: usize,
        returned: bool,
        drops: Option<usize>,
    },
}

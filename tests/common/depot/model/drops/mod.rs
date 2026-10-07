use std::{
    cell::{self, Cell},
    cmp::Ordering,
    ops::ControlFlow,
};

pub use block::Block;
use o3::depot::array::init;

use crate::common::Counted;

mod block;

#[derive(Debug)]
pub struct Tracked<'a> {
    order: u8,
    _drops: Counted<&'a Cell<Option<usize>>>,
}

impl<'a> Tracked<'a> {
    pub fn try_new(order: u8, drops: &'a [Cell<Option<usize>>]) -> Option<Self> {
        Some(Self {
            order,
            _drops: Counted(drops.get(order as usize)?),
        })
    }
}

impl PartialEq for Tracked<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
    }
}

impl Eq for Tracked<'_> {}

impl PartialOrd for Tracked<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tracked<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order.cmp(&other.order)
    }
}

#[derive(Debug)]
pub struct Order<'a>(pub &'a Cell<Option<u64>>, pub u64);

impl Drop for Order<'_> {
    fn drop(&mut self) {
        self.0.set(
            self.0
                .get()
                .and_then(|order| order.checked_mul(10)?.checked_add(self.1)),
        );
    }
}

impl<'a> Order<'a> {
    pub fn fill<const REVERSE: bool>(
        mut guard: init::Guard<'_, Self, 3, REVERSE>,
        order: &'a Cell<Option<u64>>,
        fail: bool,
    ) -> Result<[Self; 3], u64> {
        loop {
            let vacant = match guard.next() {
                ControlFlow::Break(full) => return Ok(full.into_array()),
                ControlFlow::Continue(vacant) => vacant,
            };
            let value = *vacant.get(&[1, 2, 3]);
            if fail && value == 3 {
                return Err(value);
            }
            guard = vacant.insert(Self(order, value));
        }
    }
}

#[derive(Debug)]
pub struct Payload<'a> {
    pub id: u64,
    pub drops: Counted<&'a Cell<Option<usize>>>,
}

#[derive(Debug, Default)]
pub struct Deferred<'a>(pub Cell<Option<&'a Cell<Option<usize>>>>);

impl Drop for Deferred<'_> {
    fn drop(&mut self) {
        CALLS.with(|calls| {
            let drops = self.0.get().unwrap_or(calls);
            drops.set(drops.get().and_then(|count| count.checked_add(1)));
        });
    }
}

#[repr(align(64))]
#[derive(Debug, Default)]
pub struct Aligned<'a>(pub Deferred<'a>);

std::thread_local! {
    pub static CALLS: cell::Cell<Option<usize>> = const { cell::Cell::new(Some(0)) };
}
#[derive(Debug)]
pub struct Seed;

impl Default for Seed {
    fn default() -> Self {
        CALLS.with(|calls| calls.set(calls.get().and_then(|count| count.checked_add(1))));
        Self
    }
}

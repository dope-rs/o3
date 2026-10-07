use std::{cell::Cell, mem, num, pin};

use o3::{
    base::cell::{brand, region},
    buffer,
    buffer::{
        resident,
        storage::provided::{self, account, fixed},
    },
};

use crate::common::Counted;

#[derive(PartialEq)]
pub enum Phase {
    Active,
    Closed,
}

pub struct Terminal<'a> {
    pub phase: Phase,
    pub items: [Option<Item<'a>>; 2],
    pub returned: usize,
    pub error: Option<Error>,
}

pub struct Run<'a> {
    pub drops: &'a Cell<Option<usize>>,
}

pub struct Reuse<'a> {
    pub drops: &'a Cell<Option<usize>>,
}

pub struct Closed;

#[derive(Debug)]
pub enum Error {
    Budget(resident::Reuse),
    Full(fixed::Full),
    Account(account::Cause),
    ExpectedFull,
    Release {
        closed: bool,
        index: usize,
        vacant: Option<bool>,
    },
}

impl<'a> fixed::Operation<resident::Domain, Item<'a>, Context> for Run<'a> {
    type Output = Result<(usize, usize), Error>;
    type Terminal = Terminal<'a>;
    type Proof = Closed;

    fn run<'d>(
        self,
        _: brand::Token<brand::Identity<'d>>,
        regions: (
            region::Token<region::Identity<'d>>,
            region::Token<region::Identity<'d, 1>>,
        ),
        root: provided::Root<'d, Item<'a>, Context, ()>,
        domain: pin::Pin<&'d mut resident::Domain>,
        mut terminal: fixed::Loan<'d, Self::Terminal>,
    ) -> (Self::Output, fixed::End<'d, Self::Terminal, Closed>)
    where
        Self: 'd,
    {
        let result = (|| {
            let budget = domain
                .get_mut()
                .try_budget(8, regions.0.proof())
                .map_err(Error::Budget)?;
            for bytes in [*b"held", *b"live"] {
                let vacant = root.try_vacant().map_err(Error::Full)?;
                let item = (bytes, Counted(self.drops));
                // SAFETY: Context's layout is exactly this item's initialized four bytes.
                let owner = unsafe { buffer::raw::Fixed::insert(vacant, item) };
                let held = owner
                    .try_account(&budget)
                    .map_err(|rejected| Error::Account(rejected.into_parts().1))?;
                mem::forget(held);
            }
            Ok((budget.resident(), terminal.returned))
        })();
        terminal.phase = Phase::Closed;
        (result, terminal.finish(Closed))
    }
}

impl<'a> fixed::Operation<resident::Domain, Item<'a>, Context> for Reuse<'a> {
    type Output = Result<([account::Cause; 2], [usize; 4], bool), Error>;
    type Terminal = Terminal<'a>;
    type Proof = Closed;

    fn run<'d>(
        self,
        _: brand::Token<brand::Identity<'d>>,
        regions: (
            region::Token<region::Identity<'d>>,
            region::Token<region::Identity<'d, 1>>,
        ),
        root: provided::Root<'d, Item<'a>, Context, ()>,
        domain: pin::Pin<&'d mut resident::Domain>,
        mut terminal: fixed::Loan<'d, Self::Terminal>,
    ) -> (Self::Output, fixed::End<'d, Self::Terminal, Closed>)
    where
        Self: 'd,
    {
        let result = (|| {
            let budget = domain
                .get_mut()
                .try_budget(8, regions.0.proof())
                .map_err(Error::Budget)?;
            let first = root.try_vacant().map_err(Error::Full)?;
            let second = root.try_vacant().map_err(Error::Full)?;
            // SAFETY: Context projects the complete initialized four-byte items.
            let first =
                unsafe { buffer::raw::Fixed::insert(first, (*b"held", Counted(self.drops))) }
                    .into_shared();
            // SAFETY: this independent vacancy retains the same fixed layout.
            let second =
                unsafe { buffer::raw::Fixed::insert(second, (*b"live", Counted(self.drops))) }
                    .into_shared();
            let held = first.try_account(&budget).map_err(Error::Account)?;
            let alias = held
                .try_clone()
                .map_err(|error| Error::Account(account::Cause::Retain(error)))?;
            let initial = second
                .try_account(&budget)
                .err()
                .ok_or(Error::ExpectedFull)?;
            drop(held);
            let retained = budget.resident();
            let cloned = second
                .try_account(&budget)
                .err()
                .ok_or(Error::ExpectedFull)?;
            drop(alias);
            let released = budget.resident();
            let returned = root.has_returned();
            let held = second.try_account(&budget).map_err(Error::Account)?;
            let admitted = budget.resident();
            drop(held);
            Ok((
                [initial, cloned],
                [retained, released, admitted, budget.resident()],
                returned,
            ))
        })();
        terminal.phase = Phase::Closed;
        (result, terminal.finish(Closed))
    }
}

impl<'a> fixed::Release<Terminal<'a>, Item<'a>> for Closed {
    fn release(&self, terminal: &mut Terminal<'a>, item: Item<'a>) {
        let index = terminal.returned;
        let vacant = terminal.items.get_mut(index).map(|slot| {
            terminal.returned += 1;
            slot.replace(item).is_none()
        });
        let closed = terminal.phase == Phase::Closed;
        if !closed || vacant != Some(true) {
            terminal.error.get_or_insert(Error::Release {
                closed,
                index,
                vacant,
            });
        }
    }
}

type Item<'a> = ([u8; 4], Counted<&'a Cell<Option<usize>>>);

pub struct Context;

impl buffer::raw::Fixed<Item<'_>> for Context {
    fn resident(&self) -> num::NonZero<u32> {
        num::NonZero::<u32>::MIN.saturating_add(3)
    }
    unsafe fn project<'a, 'id>(
        &'a self,
        item: &'a mut Item<'_>,
        limit: buffer::raw::Limit<'id>,
    ) -> buffer::raw::Projection<'a, 'id> {
        // SAFETY: this fixed item owns exactly the four bytes of the resident ceiling.
        unsafe { limit.project(&mut item.0) }
    }
}

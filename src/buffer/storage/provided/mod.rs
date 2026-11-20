//! Owner-preserving storage for resources returned by an external subsystem.
//! Fixed and exact capacity owners over one private intrusive state machine.

use std::{borrow::Borrow, mem, num::NonZero, ops};

pub use arena::{Arena, account::Account, owner::Owner, root::Root, vacant::Vacant};

use crate::{
    buffer,
    buffer::{
        resident, storage,
        storage::provided,
        write::{consume, consume::Consume},
    },
};

pub mod account;
mod arena;
pub mod fixed;
pub mod view;

/// Mutable provided storage preserving its owner through splitting and freezing.
pub trait Mutable<'d>: buffer::raw::Split + buffer::raw::Freeze<Output = Self::Frozen> {
    type Account: AsRef<[u8]> + buffer::Residency;
    type Frozen: Frozen<'d, Account = Self::Account>;
    type Prefix<'a>: Mutable<'d, Account = Self::Account>
    where
        Self: 'a;

    fn try_take(&mut self, amount: NonZero<usize>) -> Result<Self::Prefix<'_>, consume::Error>;
}

/// Frozen provided storage retaining its original backing.
pub trait Frozen<'d>: buffer::raw::Source + buffer::Residency {
    type Account: AsRef<[u8]> + buffer::Residency;
    fn try_account(&self, budget: &resident::Budget<'d>) -> Result<Self::Account, account::Cause>;
}

#[derive(Debug)]
#[repr(C)]
pub struct Slice<B, O> {
    bytes: B,
    owner: O,
}

/// Exclusive mutable projection into one provided item.
pub type Unique<'d> = Slice<&'d mut [u8], Owner<'d>>;

/// A removed prefix borrowing its original unique owner.
pub type Prefix<'a, 'd, B = &'a mut [u8]> = Slice<B, &'a Owner<'d>>;

impl<'d> Unique<'d> {
    pub(in crate::buffer::storage::provided) const fn new(
        view: buffer::raw::Projection<'d, '_>,
        owner: provided::Owner<'d>,
    ) -> Self {
        Self {
            bytes: view.into_inner(),
            owner,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.bytes
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.bytes
    }

    pub fn try_take(
        &mut self,
        amount: NonZero<usize>,
    ) -> Result<provided::Prefix<'_, 'd>, consume::Error> {
        Prefix::take_from(&mut self.bytes, &self.owner, amount)
    }

    pub fn chunks_mut<'a>(
        &'a mut self,
        size: NonZero<usize>,
    ) -> impl Iterator<Item = Prefix<'a, 'd>> {
        let owner = &self.owner;
        buffer::bytes::ChunksMut::new(mem::take(&mut self.bytes), size)
            .map(move |bytes| Prefix { bytes, owner })
    }

    pub fn try_account(
        self,
        budget: &resident::Budget<'d>,
    ) -> Result<
        storage::Shared<Option<provided::Account<'d>>>,
        crate::mem::Rejected<provided::Unique<'d>, account::Cause>,
    > {
        match (provided::Prefix {
            bytes: self.as_slice(),
            owner: &self.owner,
        })
        .try_account(budget)
        {
            Ok(owner) => Ok(owner),
            Err(error) => Err(crate::mem::Rejected::new(self, error)),
        }
    }

    pub fn into_shared(self) -> storage::Shared<provided::Owner<'d>> {
        // SAFETY: Unique transfers the initialized slice with the external edge that retains it.
        unsafe { storage::Shared::assume(self.bytes, self.owner) }
    }
}

// SAFETY: Unique retains the exact exclusive slice and its provided owner.
unsafe impl buffer::raw::Consume for Unique<'_> {
    fn consume<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        // SAFETY: projection retains this owner's unchanged exclusive byte extent.
        <&mut [u8] as buffer::raw::Consume>::consume(unsafe {
            prefix.map(|source| &mut source.bytes)
        });
    }
}

// SAFETY: narrowing this disjoint slice keeps its owned or borrowed slot edge, even when empty.
unsafe impl<O> buffer::raw::Store for Slice<&mut [u8], O> {
    fn empty() -> Option<Self> {
        None
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        // SAFETY: the selection belongs to these bytes and the owner stays in place.
        let slice = unsafe { slice.map(|source| &mut source.bytes) };
        <&mut [u8] as buffer::raw::Store>::project(slice);
    }
}

impl<B: AsRef<[u8]>, O> AsRef<[u8]> for Slice<B, O> {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl<O> AsMut<[u8]> for Slice<&mut [u8], O> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.bytes
    }
}

impl ops::Deref for Unique<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.bytes
    }
}

impl<'d, B, O: Borrow<Owner<'d>>> buffer::Residency for Slice<B, O> {
    fn resident(&self) -> usize {
        self.owner.borrow().resident()
    }
}

impl Consume for Unique<'_> {
    fn remaining(&self) -> usize {
        self.bytes.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        self.bytes.try_consume(amount)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        self.bytes.consume_max(max)
    }
}

const _: () = {
    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<Unique<'static>>() == 3 * word);
    assert!(
        mem::size_of::<crate::mem::Rejected<provided::Unique<'static>, account::Cause>>()
            == 6 * word
    );
    assert!(
        mem::size_of::<
            Result<
                storage::Shared<Option<provided::Account<'static>>>,
                crate::mem::Rejected<provided::Unique<'static>, account::Cause>,
            >,
        >() == 6 * word
    );
};

impl<'a, 'd> Prefix<'a, 'd> {
    fn take_from<'bytes: 'a>(
        bytes: &mut &'bytes mut [u8],
        owner: &'a Owner<'d>,
        amount: NonZero<usize>,
    ) -> Result<Self, consume::Error> {
        let bytes = buffer::raw::Selection::try_new(bytes, amount.get())?.take();
        Ok(Self { bytes, owner })
    }
}

impl<'a, 'd> Mutable<'d> for Prefix<'a, 'd> {
    type Account = storage::Shared<Option<provided::Account<'d>>>;
    type Frozen = Prefix<'a, 'd, &'a [u8]>;
    type Prefix<'b>
        = Prefix<'a, 'd>
    where
        Self: 'b;

    fn try_take(&mut self, amount: NonZero<usize>) -> Result<Self::Prefix<'_>, consume::Error> {
        Prefix::take_from(&mut self.bytes, self.owner, amount)
    }
}

impl<'a, 'd> From<Prefix<'a, 'd>> for Prefix<'a, 'd, &'a [u8]> {
    fn from(prefix: Prefix<'a, 'd>) -> Self {
        let Prefix { bytes, owner } = prefix;
        Self { bytes, owner }
    }
}

impl<'a, 'd> Frozen<'d> for Prefix<'a, 'd, &'a [u8]> {
    type Account = storage::Shared<Option<provided::Account<'d>>>;

    fn try_account(
        &self,
        budget: &resident::Budget<'d>,
    ) -> Result<storage::Shared<Option<provided::Account<'d>>>, account::Cause> {
        match self.try_account_with(budget, ())? {
            ops::ControlFlow::Continue(owner) => Ok(owner),
            ops::ControlFlow::Break(never) => match never {},
        }
    }
}

impl<'a, 'd> Prefix<'a, 'd, &'a [u8]> {
    pub(in crate::buffer) fn from_view(view: buffer::View<'a, storage::Shared<Owner<'d>>>) -> Self {
        Self {
            bytes: view.as_slice(),
            owner: view.source.owner(),
        }
    }

    #[inline]
    pub(in crate::buffer) fn try_account_with<A: resident::Admission>(
        &self,
        budget: &resident::Budget<'d>,
        admission: A,
    ) -> Result<
        ops::ControlFlow<A::Declined, storage::Shared<Option<provided::Account<'d>>>>,
        account::Cause,
    > {
        let Ok(prefix) = storage::NonEmpty::try_new(Self { ..*self }) else {
            return Ok(ops::ControlFlow::Continue(storage::Shared::new()));
        };
        prefix.try_account(*budget, admission)
    }
}

const _: () = {
    assert!(size_of::<Prefix<'static, 'static>>() == 3 * size_of::<usize>());
    assert!(size_of::<Prefix<'static, 'static, &'static [u8]>>() == 3 * size_of::<usize>());
    assert!(
        size_of::<Result<Prefix<'static, 'static>, consume::Error>>()
            == size_of::<Prefix<'static, 'static>>()
    );
};

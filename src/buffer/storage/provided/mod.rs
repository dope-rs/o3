//! Owner-preserving storage for resources returned by an external subsystem.
//! Fixed and exact capacity owners over one private intrusive state machine.

use std::{mem, num::NonZero, ops};

pub(in crate::buffer) use arena::charge::Charge;
pub use arena::{Arena, account::Account, owner::Owner, root::Root, vacant::Vacant};
use mode::Mode;

use crate::{
    buffer,
    buffer::{
        bytes::ChunksMut,
        resident, storage,
        storage::provided,
        write::{consume, consume::Consume},
    },
};

pub mod account;
mod arena;
pub mod fixed;
mod mode;
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

/// Exclusive mutable projection into one provided item.
#[derive(Debug)]
#[repr(C)]
pub struct Unique<'d> {
    pub(in crate::buffer::storage::provided) view: &'d mut [u8],
    pub(in crate::buffer::storage::provided) owner: provided::Owner<'d>,
}

impl<'d> Unique<'d> {
    pub(in crate::buffer) fn inner(&self) -> &&'d mut [u8] {
        &self.view
    }

    /// # Safety
    /// The caller preserves the projection's exact owner and initialized range.
    pub(in crate::buffer) unsafe fn inner_mut(&mut self) -> &mut &'d mut [u8] {
        &mut self.view
    }

    pub(in crate::buffer::storage::provided) const fn new(
        view: buffer::raw::Projection<'d, '_>,
        owner: provided::Owner<'d>,
    ) -> Self {
        Self {
            view: view.into_inner(),
            owner,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.view
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        self.view
    }

    pub fn try_take(
        &mut self,
        amount: NonZero<usize>,
    ) -> Result<provided::Prefix<'_, 'd>, consume::Error> {
        let bytes = buffer::raw::Selection::try_new(&mut self.view, amount.get())?.take();
        Ok(provided::Prefix {
            bytes,
            owner: &self.owner,
        })
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
        let Self { view, owner } = self;
        // SAFETY: this Unique consumes the exact projection and live owner
        // paired by its branded admission, retaining the source lifetime.

        unsafe { storage::Shared::assume(view, owner) }
    }
}

impl AsRef<[u8]> for Unique<'_> {
    fn as_ref(&self) -> &[u8] {
        self.view
    }
}

impl AsMut<[u8]> for Unique<'_> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.view
    }
}

impl ops::Deref for Unique<'_> {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.view
    }
}

impl buffer::Residency for Unique<'_> {
    fn resident(&self) -> usize {
        self.owner.resident()
    }
}

impl Consume for Unique<'_> {
    fn remaining(&self) -> usize {
        self.view.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        self.view.try_consume(amount)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        self.view.consume_max(max)
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

/// A removed prefix borrowing its original unique owner.
pub struct Prefix<'a, 'd, T = &'a mut [u8]> {
    pub(in crate::buffer) bytes: T,
    pub(in crate::buffer) owner: &'a provided::Owner<'d>,
}

impl<'a, 'd> Mutable<'d> for Prefix<'a, 'd> {
    type Account = storage::Shared<Option<provided::Account<'d>>>;
    type Frozen = Prefix<'a, 'd, &'a [u8]>;
    type Prefix<'b>
        = Prefix<'a, 'd>
    where
        Self: 'b;

    fn try_take(&mut self, amount: NonZero<usize>) -> Result<Self::Prefix<'_>, consume::Error> {
        let bytes = buffer::raw::Selection::try_new(&mut self.bytes, amount.get())?.take();
        Ok(Prefix {
            bytes,
            owner: self.owner,
        })
    }
}

impl<'a, 'd> From<Prefix<'a, 'd>> for Prefix<'a, 'd, &'a [u8]> {
    fn from(prefix: Prefix<'a, 'd>) -> Self {
        let Prefix { bytes, owner } = prefix;
        Self { bytes, owner }
    }
}

/// Disjoint mutable segments retaining their original provided owner.
pub struct Chunks<'a, 'd> {
    bytes: ChunksMut<'a>,
    owner: &'a provided::Owner<'d>,
}

impl<'a, 'd> Chunks<'a, 'd> {
    pub fn new(source: &'a mut Unique<'d>, size: NonZero<usize>) -> Self {
        Self {
            bytes: ChunksMut::new(mem::take(&mut source.view), size),
            owner: &source.owner,
        }
    }
}

impl<'a, 'd> Iterator for Chunks<'a, 'd> {
    type Item = Prefix<'a, 'd>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(Prefix {
            bytes: self.bytes.next()?,
            owner: self.owner,
        })
    }
}

impl<'a, 'd> Frozen<'d> for Prefix<'a, 'd, &'a [u8]> {
    type Account = storage::Shared<Option<provided::Account<'d>>>;

    fn try_account(
        &self,
        budget: &resident::Budget<'d>,
    ) -> Result<storage::Shared<Option<provided::Account<'d>>>, account::Cause> {
        if self.bytes.is_empty() {
            return Ok(storage::Shared::<Option<provided::Account<'_>>>::new());
        }
        use arena::Entry;

        let owner = Entry::try_account(self.owner, *budget)?;
        // SAFETY: this prefix's private construction and disjoint projections
        // retain its original owner; admission extends that exact backing.
        let inner = unsafe { storage::Shared::assume(self.bytes, Some(owner)) };
        Ok(inner)
    }
}

impl<T: AsRef<[u8]>> AsRef<[u8]> for Prefix<'_, '_, T> {
    fn as_ref(&self) -> &[u8] {
        self.bytes.as_ref()
    }
}

impl AsMut<[u8]> for Prefix<'_, '_> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.bytes
    }
}

impl buffer::Residency for Prefix<'_, '_, &[u8]> {
    fn resident(&self) -> usize {
        self.owner.resident()
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

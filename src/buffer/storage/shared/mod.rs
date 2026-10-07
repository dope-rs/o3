use crate::buffer::{
    pool::owners,
    storage::{provided::Frozen as _, regions},
};
pub(in crate::buffer::storage) mod str;

use std::{ops, ptr, vec};

use storage::{backing, provided, provided::account, range};

use crate::{
    buffer,
    buffer::{bytes, pool, resident, storage, write::consume},
    mem::rc,
};

/// One shared initialized byte extent inseparably paired with its owner.
#[derive(Debug)]
#[repr(transparent)]
pub struct Shared<O = Option<regions::Tag>> {
    pub(in crate::buffer) region: regions::Value<O>,
}

impl<O> Shared<O> {
    pub(in crate::buffer) const fn empty(owner: O) -> Self {
        Self {
            region: regions::Value::empty(owner),
        }
    }

    /// Pairs a raw initialized extent with its transferred owner.
    /// # Safety
    /// `owner` must keep `ptr..ptr + len` initialized, live, and immutable.
    pub(in crate::buffer) unsafe fn from_raw(ptr: ptr::NonNull<u8>, len: usize, owner: O) -> Self {
        Self {
            // SAFETY: the caller transfers the owner of this initialized extent.
            region: unsafe { regions::Value::new(owner, regions::Extent::new(ptr, len)) },
        }
    }

    /// Pairs a proven initialized slice with its transferred owner.
    /// # Safety
    /// `owner` must keep `bytes` initialized, live, and immutable while the buffer exists.
    pub(in crate::buffer) unsafe fn assume(bytes: &[u8], owner: O) -> Self {
        let ptr = ptr::NonNull::from(bytes).cast();
        // SAFETY: the caller proves that owner retains this initialized slice.
        unsafe { Self::from_raw(ptr, bytes.len(), owner) }
    }

    pub fn try_split<const N: usize>(mut self) -> Result<([u8; N], Self), consume::Error>
    where
        O: regions::End,
    {
        let (head, prefix) = buffer::raw::Selection::try_read(&mut self)?;
        prefix.consume();
        Ok((head, self))
    }
}

impl From<Shared<crate::buffer::raw::Rc>> for Shared<Option<regions::Tag>> {
    fn from(buf: Shared<crate::buffer::raw::Rc>) -> Self {
        Self {
            region: buf.region.into(),
        }
    }
}
impl<L> From<Shared<owners::Edge<owners::Plain<L>>>> for Shared<Option<regions::Tag>> {
    fn from(buf: Shared<owners::Edge<owners::Plain<L>>>) -> Self {
        Self {
            region: buf.region.into(),
        }
    }
}
impl<'pool, L> From<Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'pool>>>>
    for Shared<owners::Edge<owners::Plain<L>, pool::Owned>>
{
    fn from(buf: Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'pool>>>) -> Self {
        Self {
            region: buf.region.into(),
        }
    }
}

impl<O: buffer::raw::Retain> Shared<O> {
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            region: self.region.try_clone()?,
        })
    }
}

impl<O: buffer::raw::Retain> buffer::View<'_, Shared<O>> {
    pub(in crate::buffer) fn try_clone(&self) -> Result<Shared<O>, rc::Error> {
        Ok(Shared {
            // SAFETY: the source-bound selection retains the same immutable
            // region throughout retention; no independent owner can be supplied.
            region: unsafe { self.source().region.try_view(self.as_slice())? },
        })
    }
}

impl<O> AsRef<[u8]> for Shared<O> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

// SAFETY: the paired owner retains the initialized extent until consumption.
unsafe impl<O: regions::End> buffer::raw::Consume for Shared<O> {
    fn consume<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        buffer::raw::Store::advance(prefix);
    }
}

impl<O: regions::End> Shared<O> {
    pub fn project(
        mut self,
        visible: ops::Range<usize>,
    ) -> Result<Self, crate::mem::Rejected<Self, range::Error>> {
        match self.try_restrict(visible) {
            Ok(()) => Ok(self),
            Err(error) => Err(crate::mem::Rejected::new(self, error)),
        }
    }
}
impl<O> ops::Deref for Shared<O> {
    type Target = regions::Value<O>;
    fn deref(&self) -> &Self::Target {
        &self.region
    }
}
impl<O> ops::DerefMut for Shared<O> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.region
    }
}
impl<O: regions::End> consume::Consume for Shared<O> {
    fn remaining(&self) -> usize {
        self.region.len()
    }
    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        buffer::raw::Selection::try_new(self, amount).map(buffer::raw::Selection::consume)
    }
    fn consume_max(&mut self, max: usize) -> usize {
        buffer::raw::Store::advance_max(self, max)
    }
}
impl<O: buffer::Residency> buffer::Residency for Shared<O> {
    fn resident(&self) -> usize {
        self.owner().resident()
    }
}

impl<O> Shared<Option<O>> {
    pub const fn new() -> Self {
        Self::empty(None)
    }
}

impl TryFrom<&mut vec::Vec<u8>> for Shared {
    type Error = crate::alloc::Exhausted;
    fn try_from(source: &mut vec::Vec<u8>) -> Result<Self, Self::Error> {
        Ok(Self {
            region: regions::Header::try_new(source)?,
        })
    }
}
impl Shared {
    pub fn try_copy(bytes: &[u8]) -> Result<Self, storage::AllocError> {
        storage::Exact::try_copy(bytes).map(storage::Exact::into_shared)
    }
    pub fn try_get(&self, range: impl ops::RangeBounds<usize>) -> Result<Option<Self>, rc::Error> {
        Ok(self.region.try_get(range)?.map(|region| Self { region }))
    }

    #[doc(hidden)]
    pub fn backing(&self) -> Option<backing::View<'_, '_>> {
        backing::View::new(&self.region)
    }
    pub fn clear(&mut self) {
        *self = Self::new();
    }
}
impl Default for Shared {
    fn default() -> Self {
        Self::new()
    }
}
impl bytes::TryRetain for Shared {
    fn try_retain(self) -> Result<Self, storage::AllocError> {
        Ok(self)
    }
}

impl<L> storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'_>>> {
    pub(in crate::buffer) fn into_owner(self) -> storage::Shared<Option<regions::Tag>> {
        let inner: storage::Shared<owners::Edge<owners::Plain<L>>> = self.into();
        inner.into()
    }
}

impl<L> bytes::TryRetain for storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'_>>> {
    fn try_retain(self) -> Result<storage::Shared, storage::AllocError> {
        Ok(self.into_owner())
    }
}

impl<'d> storage::Shared<provided::Owner<'d>> {
    /// Checks admission against this source's exact budget lifetime.
    pub fn try_account(
        &self,
        budget: &resident::Budget<'d>,
    ) -> Result<storage::Shared<Option<provided::Account<'d>>>, account::Cause> {
        provided::Prefix {
            bytes: self.as_ref(),
            owner: self.owner(),
        }
        .try_account(budget)
    }
    pub fn try_get(
        &self,
        range: ops::Range<usize>,
        budget: &resident::Budget<'d>,
    ) -> Result<storage::Shared<Option<provided::Account<'d>>>, account::Error> {
        let Some(bytes) = self.as_ref().get(range.clone()) else {
            return Err(account::Error::Range(range::Error::new(range, self.len())));
        };
        provided::Prefix {
            bytes,
            owner: self.owner(),
        }
        .try_account(budget)
        .map_err(account::Error::Account)
    }
}

// SAFETY: the paired owner retains this initialized extent and its empty-storage policy.
unsafe impl<O: regions::End> buffer::raw::Store for Shared<O> {
    fn empty() -> Option<Self> {
        O::EMPTY.map(Self::empty)
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        let (target, range) = slice.into_parts();
        // SAFETY: the exclusive operation retains this owner's selected initialized extent.
        unsafe { target.region.restrict(range.into_inner()) };
    }
}

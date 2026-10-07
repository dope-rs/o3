use std::{num::NonZero, ops};

use provided::account;

use crate::{
    base::num::bounded::U32,
    buffer,
    buffer::{
        bytes::bounded,
        hold, resident, storage,
        storage::{provided, range},
        write::consume,
    },
    mem::rc,
};

/// Value whose stable extent fits a signed 32-bit count.
///
/// Admission requires an extent stable through shared access.
#[derive(Debug)]
#[repr(transparent)]
pub struct Value<T>(pub(in crate::buffer) T);

impl<const N: usize> Value<[u8; N]> {
    pub const fn new(value: [u8; N]) -> Self {
        const { assert!(N <= bounded::MAX) };
        Self(value)
    }
}

impl<T: buffer::raw::Source> Value<T> {
    pub fn try_new(value: T) -> Result<Self, bounded::Rejected<T>> {
        if value.as_ref().len() > bounded::MAX {
            return Err(bounded::Rejected::new(value));
        }
        Ok(Self(value))
    }
}

impl<'d> Value<provided::Unique<'d>> {
    pub fn try_take(
        &mut self,
        amount: NonZero<usize>,
    ) -> Result<Value<provided::Prefix<'_, 'd>>, consume::Error> {
        Ok(Value(self.0.try_take(amount)?))
    }
}

impl<T: buffer::raw::Mutable> AsMut<[u8]> for Value<T> {
    fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut()
    }
}

impl<'d> From<Value<provided::Unique<'d>>> for Value<storage::Shared<provided::Owner<'d>>> {
    fn from(bytes: Value<provided::Unique<'d>>) -> Self {
        Value(bytes.0.into_shared())
    }
}

impl<T> Value<T> {
    /// Transfers storage while preserving its admitted extent.
    pub fn into_storage<U>(self) -> Value<U>
    where
        T: buffer::raw::Convert<U>,
    {
        Value(buffer::raw::Convert::into(self.0))
    }

    /// Seals the original producer's extent proof into its bytes.
    pub fn from_raw(proof: bounded::raw::Input<T>) -> Self {
        Self(proof.into_inner())
    }

    #[must_use]
    pub fn into_inner(self) -> T {
        self.0
    }
}

const _: () = assert!(size_of::<Value<[u8; 1]>>() == 1);

impl<T: AsRef<[u8]>> AsRef<[u8]> for Value<T> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<T: AsRef<[u8]>> From<&Value<T>> for U32<0, { bounded::MAX as u32 }> {
    fn from(bytes: &Value<T>) -> Self {
        bounded::raw::Input::count(bytes)
    }
}

impl<T: buffer::Residency> buffer::Residency for Value<T> {
    fn resident(&self) -> usize {
        self.0.resident()
    }
}

impl<'d> Value<storage::Shared<provided::Owner<'d>>> {
    pub fn try_account(
        &self,
        budget: &resident::Budget<'d>,
    ) -> Result<Value<storage::Shared<Option<provided::Account<'d>>>>, account::Cause> {
        Ok(Value(self.0.try_account(budget)?))
    }

    pub fn try_get(
        &self,
        range: ops::Range<usize>,
        budget: &resident::Budget<'d>,
    ) -> Result<Value<storage::Shared<Option<provided::Account<'d>>>>, account::Error> {
        Ok(Value(self.0.try_get(range, budget)?))
    }
}

impl<'a, T: buffer::Read<'a>> buffer::Read<'a> for Value<T>
where
    Self: bounded::Source,
{
    fn as_slice(&self) -> &'a [u8] {
        self.0.as_slice()
    }

    fn try_split<const N: usize>(self) -> Result<(&'a [u8; N], Self), consume::Error> {
        let (head, tail) = self.0.try_split()?;
        Ok((head, Self(tail)))
    }
}

impl<T: buffer::raw::Store + buffer::raw::Source> consume::Consume for Value<T> {
    fn remaining(&self) -> usize {
        self.0.as_ref().len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        buffer::raw::Selection::try_new(&mut self.0, amount).map(buffer::raw::Selection::consume)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        buffer::raw::Store::advance_max(&mut self.0, max)
    }
}

impl Default for Value<storage::Shared<Option<provided::Account<'_>>>> {
    fn default() -> Self {
        Value(storage::Shared::<Option<provided::Account<'_>>>::new())
    }
}

impl<'d> Value<storage::Shared<Option<provided::Account<'d>>>> {
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Value(self.0.try_clone()?))
    }

    pub fn project(
        self,
        range: ops::Range<usize>,
    ) -> Result<Self, crate::mem::Rejected<Self, range::Error>> {
        match self.0.project(range) {
            Ok(value) => Ok(Value(value)),
            Err(error) => Err(error.map(Value)),
        }
    }
}

impl Value<&[u8]> {
    pub fn try_get(&self, range: ops::Range<usize>) -> Result<Self, range::Error> {
        use buffer::Cursor as _;
        Ok(Value(self.0.try_get(range)?))
    }
}

impl bounded::Source for Value<&[u8]> {}

const _: () = {
    assert!(size_of::<Value<&[u8]>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Value<storage::Shared<Option<provided::Account<'static>>>>>()
            == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Option<Value<storage::Shared<Option<provided::Account<'static>>>>>>()
            == 3 * size_of::<usize>()
    );
};

impl<O: Unpin> buffer::Source for Value<storage::Shared<O>> {
    type View<'input>
        = Value<buffer::View<'input, storage::Shared<O>>>
    where
        Self: 'input;

    fn remaining(&self) -> usize {
        self.0.as_ref().len()
    }
}

// SAFETY: The bounded view delegates its exact immutable range to the provided owner.
unsafe impl<'d> hold::raw::Retain<'d> for Value<storage::Shared<provided::Owner<'d>>> {
    fn charge<'a>(plan: &Self::View<'a>) -> hold::Charge
    where
        Self: 'a,
    {
        <storage::Shared<provided::Owner<'d>> as hold::raw::Retain<'d>>::charge(&plan.0)
    }

    fn try_copy<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, hold::Error>
    where
        Self: 'a,
    {
        Ok(resident::Shared::try_copy(plan, budget)?)
    }

    /// Retains this admitted extent with its original account and borrow domain.
    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, hold::Error>
    where
        Self: 'a,
    {
        <storage::Shared<provided::Owner<'d>> as hold::raw::Retain<'d>>::try_retain(&plan.0, budget)
    }
}

impl<'a, 'd> provided::Mutable<'d> for Value<provided::Prefix<'a, 'd>> {
    type Account = Value<storage::Shared<Option<provided::Account<'d>>>>;
    type Frozen = Value<provided::Prefix<'a, 'd, &'a [u8]>>;
    type Prefix<'b>
        = Value<provided::Prefix<'a, 'd>>
    where
        Self: 'b;

    fn try_take(&mut self, amount: NonZero<usize>) -> Result<Self::Prefix<'_>, consume::Error> {
        Ok(Self(self.0.try_take(amount)?))
    }
}

impl<'a, 'd> provided::Frozen<'d> for Value<provided::Prefix<'a, 'd, &'a [u8]>> {
    type Account = Value<storage::Shared<Option<provided::Account<'d>>>>;

    fn try_account(
        &self,
        budget: &resident::Budget<'d>,
    ) -> Result<bounded::Value<storage::Shared<Option<provided::Account<'d>>>>, account::Cause>
    {
        Ok(bounded::Value(self.0.try_account(budget)?))
    }
}

const _: () = {
    assert!(size_of::<Value<provided::Prefix<'static, 'static>>>() == 3 * size_of::<usize>());
    assert!(
        size_of::<Option<Value<provided::Prefix<'static, 'static>>>>() == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Value<provided::Prefix<'static, 'static, &[u8]>>>() == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Result<Value<provided::Prefix<'static, 'static>>, consume::Error>>()
            == 3 * size_of::<usize>()
    );
};

impl<'a, 'd> From<Value<provided::Prefix<'a, 'd>>> for Value<provided::Prefix<'a, 'd, &'a [u8]>> {
    fn from(prefix: Value<provided::Prefix<'a, 'd>>) -> Self {
        Value(prefix.0.into())
    }
}

impl<'a, S: ?Sized> Value<buffer::View<'a, S>> {
    pub fn try_get(&self, range: ops::Range<usize>) -> Result<Self, range::Error> {
        Ok(Self(self.0.try_get(range)?))
    }
}

impl<S: ?Sized> bounded::Source for Value<buffer::View<'_, S>> {}

impl<S: ?Sized> Clone for Value<buffer::View<'_, S>> {
    fn clone(&self) -> Self {
        Self(self.0.reborrow())
    }
}

impl<'d> Value<buffer::View<'_, storage::Shared<Option<provided::Account<'d>>>>> {
    pub fn try_retain(
        self,
    ) -> Result<bounded::Value<storage::Shared<Option<provided::Account<'d>>>>, rc::Error> {
        Ok(bounded::Value(self.0.try_retain()?))
    }
}

const _: () = {
    assert!(
        size_of::<Value<buffer::View<'static, storage::Shared<provided::Owner<'static>>>>>()
            == 3 * size_of::<usize>()
    );
    assert!(
        size_of::<Option<Value<buffer::View<'static, storage::Shared<provided::Owner<'static>>>>>>(
        ) == 3 * size_of::<usize>()
    );
};

impl<'d> buffer::Source for Value<provided::Unique<'d>> {
    type View<'input>
        = Value<&'input [u8]>
    where
        Self: 'input;
    fn remaining(&self) -> usize {
        self.0.as_ref().len()
    }
}

impl<'d> buffer::Cursor for Value<provided::Unique<'d>> {
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error> {
        Ok(Value(buffer::Cursor::try_get(self.0.as_ref(), range)?))
    }
}
impl<'d> buffer::Complete for Value<provided::Unique<'d>> {
    fn view(&self) -> Self::View<'_> {
        Value(self.0.as_ref())
    }
}

impl<O: Unpin> buffer::Cursor for Value<storage::Shared<O>> {
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error> {
        Ok(Value(buffer::View::new(&self.0).try_get(range)?))
    }
}
impl<O: Unpin> buffer::Complete for Value<storage::Shared<O>> {
    fn view(&self) -> Self::View<'_> {
        Value(buffer::View::new(&self.0))
    }
}

// SAFETY: consuming an exact prefix cannot enlarge the admitted range.
unsafe impl<T: buffer::raw::Consume> buffer::raw::Consume for Value<T> {
    fn consume<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        prefix.consume::<T>();
    }
}

// SAFETY: a projection only restricts the exact admitted storage extent.
unsafe impl<T: buffer::raw::Store> buffer::raw::Store for Value<T> {
    fn empty() -> Option<Self> {
        T::empty().map(Self)
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        let (target, range) = slice.into_parts();
        // SAFETY: the transparent bound preserves this owner's admitted extent.
        T::restrict(unsafe { buffer::raw::Slice::from_parts(&mut target.0, range) });
    }
}

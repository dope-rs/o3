use storage::provided;

use crate::buffer::{
    self,
    hold::{Charge, Error, ranges},
    resident, storage,
    storage::{provided::Frozen as _, regions},
};

impl<'source, 'ranges, S: buffer::raw::Source + ?Sized, N: buffer::raw::Length>
    ranges::Set<'source, 'ranges, S, N>
{
    /// # Safety
    /// Keep these admitted nonempty ranges within this source's unchanged coordinate origin.
    /// Count is <=u32::MAX and their exact sum is len; copied sources preserve all bytes.
    pub unsafe fn from_parts(source: &'source S, ranges: &'ranges [(N, N)], len: usize) -> Self {
        Self::new(source, ranges, len)
    }
}

/// # Safety
/// Source and view bytes remain unchanged through shared access and these methods; Cursor/Complete selection and Read splitting preserve exact visible ranges and prefix/suffix.
/// Successful copies/retains preserve selected bytes, order, length, and ownership for 'd; backing identity may change.
pub unsafe trait Retain<'d>: buffer::Source {
    /// Bounds retained Residency by max(plan.len(), resident_bytes), including retries.
    fn charge<'a>(plan: &Self::View<'a>) -> Charge
    where
        Self: 'a,
    {
        Charge::from_commit(buffer::Read::as_slice(plan).len(), 0, 0)
    }

    /// Copies the selected bytes with output Residency at most their length.
    fn try_copy<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, Error>
    where
        Self: 'a,
    {
        resident::Exact::try_copy(budget, buffer::Read::as_slice(plan))
            .map(resident::Exact::into_shared)
            .map_err(Error::Resident)
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, Error>
    where
        Self: 'a,
    {
        Self::try_copy(plan, budget)
    }
}

// SAFETY: The immutable provided view retains its exact range and original registration.
unsafe impl<'d> Retain<'d> for storage::Shared<provided::Owner<'d>> {
    fn charge<'a>(plan: &Self::View<'a>) -> Charge
    where
        Self: 'a,
    {
        Charge::from_view(plan)
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, Error>
    where
        Self: 'a,
    {
        let prefix = provided::Prefix {
            bytes: plan.as_slice(),
            owner: plan.source().owner(),
        };
        let owner = prefix.try_account(budget).map_err(Error::Account)?;
        Ok(resident::Shared::from(owner))
    }
}

// SAFETY: The immutable view clones its original owner and preserves its exact range.
unsafe impl<'d, O: buffer::raw::Retain + buffer::Residency + Unpin> Retain<'d>
    for storage::Shared<O>
where
    Self: Into<resident::Shared<'d>>,
{
    fn charge<'a>(plan: &Self::View<'a>) -> Charge
    where
        Self: 'a,
    {
        Charge::from_view(plan)
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        _: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, Error>
    where
        Self: 'a,
    {
        let Some(view) = regions::NonEmpty::new(plan) else {
            return Ok(resident::Shared::new());
        };
        let inner = view
            .view()
            .try_clone()
            .map_err(resident::AllocError::Retain)?;
        Ok(inner.into())
    }
}

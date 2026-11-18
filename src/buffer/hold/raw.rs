use resident::Admission as _;

use crate::buffer::{
    self,
    hold::{Error, Retained},
    resident, storage,
    storage::{provided, provided::Frozen as _, regions},
};

/// # Safety
/// Source and view bytes remain unchanged through shared access and these methods; Cursor/Complete selection and Read splitting preserve exact visible ranges and prefix/suffix.
/// Successful copies/retains preserve selected bytes, order, length, and ownership for 'd; backing identity may change.
pub unsafe trait Retain<'d>: buffer::Source {
    /// Bounds retained Residency by max(plan.len(), resident(plan)), including retries.
    fn resident<'a>(_: &Self::View<'a>) -> usize
    where
        Self: 'a,
    {
        0
    }

    /// Uniquely owns the copied bytes with output Residency at most their length.
    fn try_copy<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<storage::Owned<resident::Budget<'d>>, Error>
    where
        Self: 'a,
    {
        storage::Owned::try_from_slice(budget, buffer::Read::as_slice(plan))
            .map_err(Error::Resident)
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<Retained<'d>, Error>
    where
        Self: 'a,
    {
        Self::try_copy(plan, budget).map(Into::into)
    }

    /// Optionally retains the original backing without copying payload bytes.
    /// Existing charges require residency at most `limit`; fresh charges must also fit.
    /// A decline or error preserves the borrowed plan for fallback.
    fn try_retain_with<'a>(
        _: &Self::View<'a>,
        _: &resident::Budget<'d>,
        _: usize,
    ) -> Result<Option<Retained<'d>>, Error>
    where
        Self: 'a,
    {
        Ok(None)
    }
}

// SAFETY: The immutable provided view retains its exact range and original registration.
unsafe impl<'d> Retain<'d> for storage::Shared<provided::Owner<'d>> {
    fn resident<'a>(plan: &Self::View<'a>) -> usize
    where
        Self: 'a,
    {
        plan.resident()
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
    ) -> Result<Retained<'d>, Error>
    where
        Self: 'a,
    {
        let prefix = provided::Prefix {
            bytes: plan.as_slice(),
            owner: plan.source.owner(),
        };
        let owner = prefix.try_account(budget).map_err(Error::Account)?;
        Ok(resident::Shared::from(owner).into())
    }

    fn try_retain_with<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
        limit: usize,
    ) -> Result<Option<Retained<'d>>, Error>
    where
        Self: 'a,
    {
        let prefix = provided::Prefix {
            bytes: plan.as_slice(),
            owner: plan.source.owner(),
        };
        let std::ops::ControlFlow::Continue(owner) = prefix
            .try_account_with(budget, resident::Cap(limit))
            .map_err(Error::Account)?
        else {
            return Ok(None);
        };
        Ok(Some(resident::Shared::from(owner).into()))
    }
}

// SAFETY: The immutable view clones its original owner and preserves its exact range.
unsafe impl<'d, O: buffer::raw::Retain + buffer::Residency + Unpin> Retain<'d>
    for storage::Shared<O>
where
    Self: Into<resident::Shared<'d>>,
{
    fn resident<'a>(plan: &Self::View<'a>) -> usize
    where
        Self: 'a,
    {
        plan.resident()
    }

    fn try_retain<'a>(
        plan: &Self::View<'a>,
        _: &resident::Budget<'d>,
    ) -> Result<Retained<'d>, Error>
    where
        Self: 'a,
    {
        let Some(view) = regions::NonEmpty::new(plan) else {
            return Ok(resident::Shared::new().into());
        };
        let inner = view
            .view()
            .try_clone()
            .map_err(resident::AllocError::Retain)?;
        let inner: resident::Shared<'d> = inner.into();
        Ok(inner.into())
    }

    fn try_retain_with<'a>(
        plan: &Self::View<'a>,
        budget: &resident::Budget<'d>,
        limit: usize,
    ) -> Result<Option<Retained<'d>>, Error>
    where
        Self: 'a,
    {
        let resident = plan.as_slice().len().max(Self::resident(plan));
        if resident::Cap(limit).check(*budget, resident).is_break() {
            return Ok(None);
        }
        Self::try_retain(plan, budget).map(Some)
    }
}

use std::fmt;

use crate::{
    buffer,
    buffer::{
        hold, resident, resident::copy, segments, segments::Item, storage, storage::regions,
        write::consume,
    },
    mem::rc,
};

pub trait Seal: Sized {
    type Retention: Default + fmt::Debug;

    const EMPTY: Self;

    fn try_copy(
        source: Result<buffer::raw::Slices<'_, 1>, consume::Error>,
        domain: &Self::Retention,
    ) -> Result<Item<Self>, hold::CopyError>;
    fn try_share(view: regions::NonEmpty<'_, '_, Self>) -> Result<Item<Self>, rc::Error>;
    fn truncate(self, max: usize) -> Self;
}

impl Seal for storage::Shared {
    type Retention = ();

    const EMPTY: Self = Self::new();

    fn try_copy(
        source: Result<buffer::raw::Slices<'_, 1>, consume::Error>,
        (): &Self::Retention,
    ) -> Result<Item<Self>, hold::CopyError> {
        let source = source
            .map_err(storage::AllocError::Capacity)
            .map_err(hold::CopyError::Storage)?;
        let value = source
            .try_owned()
            .map_err(storage::AllocError::Exhausted)
            .map_err(hold::CopyError::Storage)?
            .into_shared();
        Ok(Item {
            value,
            nonempty: segments::One::new(),
        })
    }

    fn truncate(mut self, max: usize) -> Self {
        buffer::raw::Selection::new(&mut self, max).commit();
        self
    }
    fn try_share(view: regions::NonEmpty<'_, '_, Self>) -> Result<Item<Self>, rc::Error> {
        Ok(Item {
            value: view.view().try_clone()?,
            nonempty: segments::One::new(),
        })
    }
}

impl<'d> Seal for resident::Shared<'d> {
    type Retention = Option<resident::Budget<'d>>;

    const EMPTY: Self = Self::new();

    fn try_copy(
        source: Result<buffer::raw::Slices<'_, 1>, consume::Error>,
        domain: &Self::Retention,
    ) -> Result<Item<Self>, hold::CopyError> {
        let source = source
            .map_err(resident::AllocError::Capacity)
            .map_err(hold::CopyError::Resident)?;
        let budget = domain
            .as_ref()
            .ok_or(hold::CopyError::Authority(resident::BindError::Unbound))?;
        let value = source
            .try_copy(budget)
            .map_err(|cause| {
                hold::CopyError::Resident(match cause {
                    copy::Error::Budget(cause) => resident::AllocError::Budget(cause),
                    copy::Error::Allocation(cause) => resident::AllocError::Exhausted(cause),
                })
            })?
            .into_shared();
        Ok(Item {
            value,
            nonempty: segments::One::new(),
        })
    }

    fn truncate(mut self, max: usize) -> Self {
        buffer::raw::Selection::new(&mut self, max).commit();
        self
    }

    fn try_share(view: regions::NonEmpty<'_, '_, Self>) -> Result<Item<Self>, rc::Error> {
        Ok(Item {
            value: resident::Shared::try_share(view)?,
            nonempty: segments::One::new(),
        })
    }
}

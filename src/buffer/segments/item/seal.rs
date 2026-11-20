use std::fmt;

use crate::{
    buffer,
    buffer::{hold, resident, segments::Item, storage, write::consume},
    mem::rc,
};

pub trait Seal: Sized {
    type Retention: Default + fmt::Debug;

    const EMPTY: Self;

    fn try_copy(
        source: Result<buffer::raw::Slices<'_, 1>, consume::Error>,
        domain: &Self::Retention,
    ) -> Result<Item<Self>, hold::CopyError>;
    fn try_share(view: &Item<buffer::View<'_, Self>>) -> Result<Item<Self>, rc::Error>;
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
        Ok(Item { value })
    }

    fn try_share(view: &Item<buffer::View<'_, Self>>) -> Result<Item<Self>, rc::Error> {
        Ok(Item {
            value: view.value.try_clone()?,
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
            .map_err(resident::AllocError::from)
            .map_err(hold::CopyError::Resident)?
            .into_shared();
        Ok(Item { value })
    }

    fn try_share(view: &Item<buffer::View<'_, Self>>) -> Result<Item<Self>, rc::Error> {
        let view = &view.value;
        // SAFETY: the view borrows the original region and its admitted nonempty slice together.
        let region = unsafe { view.source.region.try_view(view.as_slice())? };
        Ok(Item {
            value: Self { region },
        })
    }
}

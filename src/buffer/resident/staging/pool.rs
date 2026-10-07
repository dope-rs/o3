use std::ptr;

use crate::{
    buffer::{
        pool,
        pool::{owners, state},
        resident,
        resident::{
            staging,
            staging::{Header, Mode, Plan},
        },
    },
    mem,
    mem::{credit, rc},
};

/// One retained edge to the staging allocation authority.
#[repr(transparent)]
pub struct Pool<'d> {
    inner: mem::raw::Owner<Header<'d>>,
}

impl<'d> Pool<'d> {
    pub fn try_new(
        budget: &resident::Budget<'d>,
        layout: staging::Layout,
    ) -> Result<Self, resident::AllocError> {
        Self::build(budget, layout, Mode::Local)
    }

    pub fn try_account(
        budget: &resident::Budget<'d>,
        layout: staging::Layout,
    ) -> Result<Self, resident::AllocError> {
        Self::build(budget, layout, Mode::Accounted)
    }

    fn build(
        budget: &resident::Budget<'d>,
        layout: staging::Layout,
        mode: Mode,
    ) -> Result<Self, resident::AllocError> {
        if budget.capacity() < layout.resident().get() {
            return Err(resident::AllocError::Budget(credit::Error::new(
                layout.resident(),
                budget.available(),
            )));
        }
        let warm = pool::Pool::try_account(pool::Tally::from(layout), *budget)?;
        let state = <Box<Header<'d>> as crate::alloc::BoxExt<Header<'d>>>::try_new(Header {
            refs: mem::raw::Count::one(),
            budget: *budget,
            warm,
            plan: layout.plan(),
            mode,
        })
        .map_err(resident::AllocError::Exhausted)?;
        Ok(Self {
            // SAFETY: the initialized box transfers its initial counted edge.
            inner: unsafe { mem::raw::Owner::from_raw(ptr::NonNull::from(Box::leak(state))) },
        })
    }

    pub(in crate::buffer) const fn budget(&self) -> resident::Budget<'d> {
        self.inner.get().budget
    }

    pub(in crate::buffer::resident) const fn warm(
        &self,
    ) -> &pool::Pool<state::Uninit, owners::Resident<'d>> {
        &self.inner.get().warm
    }

    pub(in crate::buffer::resident) const fn plan(&self) -> Plan {
        self.inner.get().plan
    }

    pub(in crate::buffer::resident) const fn mode(&self) -> Mode {
        self.inner.get().mode
    }
}

impl Pool<'_> {
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            inner: self.inner.try_clone()?,
        })
    }
}

const _: () = assert!(size_of::<Pool<'static>>() == size_of::<usize>());

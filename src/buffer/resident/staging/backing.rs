use std::num;

use crate::buffer::{
    bytes::window,
    pool,
    pool::owners,
    resident,
    resident::staging::{Mode, Pool, Reserve},
};

pub(super) enum Backing<'d> {
    Warm(pool::Span<owners::Resident<'d>>),
    Local(window::State<resident::Lease<'d>, { u32::MAX as usize }>),
    Accounted(pool::Span<owners::Resident<'d>>),
}

impl<'d> Backing<'d> {
    pub(super) fn try_fresh<I: crate::buffer::raw::SliceExt>(
        reserve: Reserve<&Pool<'d>, (&[u8], owners::Measured<I>)>,
    ) -> Result<Self, resident::AllocError> {
        match reserve.buf.mode() {
            Mode::Local => {
                let (pool, target, source) = reserve.into_parts();
                let state = window::Fresh::try_staged(Reserve {
                    buf: pool.budget(),
                    target,
                    source,
                })?;
                Ok(Self::Local(state))
            }
            Mode::Accounted => {
                let cursor = pool::Span::try_staged(reserve)?;
                Ok(Self::Accounted(cursor))
            }
        }
    }

    pub(super) fn capacity(&self) -> num::NonZero<u32> {
        match self {
            Self::Warm(cursor) | Self::Accounted(cursor) => cursor.capacity(),
            Self::Local(snapshot) => snapshot.capacity(),
        }
    }
}

use std::{num::NonZero, pin::Pin};

use o3::{
    alloc,
    base::cell::region,
    buffer::{
        self, Complete as _,
        bytes::bounded,
        resident,
        storage::{self, provided},
    },
};

use crate::common::buffer::provided::Error;

/// Capacity of a scoped resident test domain.
pub struct Capacity(pub usize);

impl Capacity {
    pub fn try_scope<R>(
        self,
        op: impl for<'d> FnOnce(resident::Budget<'d>) -> R,
    ) -> Result<R, resident::Reuse> {
        let mut domain = resident::Domain::new();
        region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
            let budget = domain.get_mut().try_budget(self.0, token.proof())?;
            Ok(op(budget))
        })
    }

    pub fn try_bytes<const N: usize>(
        self,
        bytes: [u8; N],
        apply: impl for<'a, 'd> FnOnce(
            bounded::Value<buffer::View<'a, storage::Shared<provided::Owner<'d>>>>,
            resident::Budget<'d>,
        ),
    ) -> Result<(usize, Option<[u8; N]>), Error> {
        let mut arena =
            provided::Arena::<_, _>::try_with(NonZero::<u32>::MIN, || Ok::<(), alloc::Error>(()))
                .map_err(Error::Allocation)?;
        let mut domain = resident::Domain::new();
        arena.scope(Pin::new(&mut domain), |token, arena, domain| {
            let budget = domain
                .get_mut()
                .try_budget(self.0, token.proof())
                .map_err(Error::Budget)?;
            let resident = {
                let slot = arena.vacant().ok_or(Error::SlotEmpty)?;
                let source = slot.try_insert(bytes, N as u32, <[u8; N]>::as_mut_slice);
                let source = source.map_err(|rejected| Error::Projection(rejected.discard()))?;

                let source = bounded::Value::try_new(source.into_shared());
                let source = source.map_err(|rejected| Error::Bounded(rejected.into_parts().1))?;
                apply(source.view(), budget);
                budget.resident()
            };
            Ok((resident, arena.pop_returned()))
        })
    }
}

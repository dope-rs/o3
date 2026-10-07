use crate::{
    buffer::{
        pool,
        pool::{owners, state},
        resident,
        resident::staging,
    },
    mem,
};

pub(in crate::buffer) struct Header<'d> {
    pub(in crate::buffer) refs: mem::raw::Count,
    pub(super) budget: resident::Budget<'d>,
    pub(super) warm: pool::Pool<state::Uninit, owners::Resident<'d>>,
    pub(super) plan: staging::Plan,
    pub(super) mode: staging::Mode,
}

//! Fixed-capacity FIFO handoff of affine grants.

use std::mem;

use crate::depot::queue::resident;

mod lane;
mod registry;

pub use lane::Lane;
pub use registry::Registry;

/// A linear registration in a [`Registry`]. Value and grant payloads are
/// invariant, so safe code cannot widen borrowed ownership.
///
#[repr(transparent)]
pub struct Registration<'target, W, G, L> {
    lease: resident::Lease<'target, Registry<W, G, L>>,
}

impl<W, G, L> Default for Registration<'_, W, G, L> {
    fn default() -> Self {
        Self::new()
    }
}

impl<W, G, L> Registration<'_, W, G, L> {
    pub const fn new() -> Self {
        Self {
            lease: resident::Lease::new(),
        }
    }

    /// Cancels the registration, retiring a pending value at its registry and
    /// returning any delivered grant to the caller.
    pub fn cancel(&mut self) -> Cancel<G> {
        let Some((registry, key)) = self.lease.active() else {
            return Cancel::Empty;
        };
        let key = *key;
        self.lease.detach();
        registry.cancel(key)
    }

    pub fn take_grant(&mut self) -> Grant<G> {
        match self.lease.active() {
            Some((registry, key)) => registry.claim(*key, self),
            None => Grant::Empty,
        }
    }
}

const _: () = {
    assert!(
        mem::size_of::<Registration<'static, usize, usize, resident::Fixed<2>>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::align_of::<Registration<'static, usize, usize, resident::Fixed<2>>>()
            == mem::align_of::<usize>()
    );
};

/// A registration whose current handoff binding rejected an update.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "a rejected registration must be handled"]
pub enum Reject {
    Active { index: usize },
    Draining { index: usize },
    Full { index: usize },
    GrantPending { index: usize },
}

/// Exact result of one grant offer.
#[derive(Eq, PartialEq)]
#[must_use = "a rejected affine grant remains caller-owned"]
pub enum Offer<W, G> {
    Value(W),
    Draining(G),
    Empty(G),
}

/// State of an affine grant retained by one registration.
#[derive(Eq, PartialEq)]
pub enum Grant<G> {
    Empty,
    Pending,
    Ready(G),
}

#[must_use = "a delivered affine grant remains caller-owned"]
#[derive(Eq, PartialEq)]
pub enum Cancel<G> {
    Empty,
    Wake,
    Grant(G),
}

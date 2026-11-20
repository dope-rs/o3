//! Fixed-capacity FIFO handoff of affine grants.

use std::mem;

use crate::depot::queue::resident::{self, raw};

/// Registry-owned FIFO lanes for one-at-a-time affine handoff.
pub type Registry<W, G, L> = resident::Lanes<raw::Entry<W, G>, resident::Parking, L>;

impl<W, G, L> resident::Lane<'_, Registry<W, G, L>> {
    pub fn offer_one(self, grant: G) -> Offer<W, G> {
        if self.owner.is_draining() {
            return Offer::Draining(grant);
        }
        let Some(entry) = self.owner.park(self.position) else {
            return Offer::Empty(grant);
        };
        // SAFETY: park unlinked the initialized wake. Its grant is installed before returning.
        let value = unsafe { entry.take() };
        entry.write_grant(grant);
        Offer::Value(value)
    }

    pub fn is_empty(self) -> bool {
        self.owner.is_empty(self.position)
    }
}

impl<W, G, L> resident::Target for Registry<W, G, L> {
    type Parked = usize;

    fn detached_grant<T>(view: raw::View<'_, T>, index: usize) -> Option<usize> {
        // The previous hook already proved detachment; only a parked grant points forward to itself.
        (view.hook().next.get() == view.key().raw()).then_some(index)
    }

    fn retire(&self, key: resident::Key) {
        if let resident::Removal::Parked(entry) = self.remove(key) {
            // SAFETY: removal owns the grant. Move it out before its destructor can reuse this slot.
            drop(unsafe { entry.take_grant() });
        }
    }
}

/// A linear registration in a [`Registry`]. Value and grant payloads are
/// invariant, so safe code cannot widen borrowed ownership.
pub type Registration<'target, W, G, L> = resident::Lease<'target, Registry<W, G, L>>;

impl<W, G, L> Registration<'_, W, G, L> {
    /// Cancels the registration, retiring a pending value at its registry and
    /// returning any delivered grant to the caller.
    pub fn cancel(&mut self) -> Cancel<G> {
        let Some((registry, key)) = self.active() else {
            return Cancel::Empty;
        };
        let key = *key;
        self.detach();
        match registry.remove(key) {
            resident::Removal::Linked => Cancel::Wake,
            // SAFETY: removing the parked registration returned its initialized grant.
            resident::Removal::Parked(entry) => Cancel::Grant(unsafe { entry.take_grant() }),
            resident::Removal::Empty => Cancel::Empty,
        }
    }

    pub fn take_grant(&mut self) -> Grant<G> {
        let Some((registry, key)) = self.active() else {
            return Grant::Empty;
        };
        let Some(entry) = registry.claim(*key) else {
            return Grant::Pending;
        };
        self.detach();
        // SAFETY: claim consumed this registration's parked grant.
        Grant::Ready(unsafe { entry.take_grant() })
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

pub type Reject = resident::Reject<usize>;

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

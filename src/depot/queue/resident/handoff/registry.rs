use std::{cell, mem};

use crate::depot::queue::resident::{
    self,
    handoff::{self, Cancel, Registration},
};

/// Registry-owned FIFO lanes for one-at-a-time affine handoff.
pub struct Registry<W, G, L> {
    pub(super) lanes: resident::Lanes<Entry<W, G>, resident::Parking, L>,
    pub(super) draining: resident::Draining,
}

impl<W, G, L> Registry<W, G, L> {
    pub fn try_new(capacity: usize, lanes: L) -> Result<Self, resident::Error>
    where
        L: resident::Extent,
    {
        Ok(Self {
            lanes: resident::Lanes::try_new(capacity, lanes)?,
            draining: resident::Draining::default(),
        })
    }

    pub fn has_waiters(&self) -> bool {
        self.lanes.has_waiters()
    }

    pub fn begin_drain(&self) {
        self.draining.begin();
    }

    pub fn is_draining(&self) -> bool {
        self.draining.is_active()
    }

    /// Takes the next value in drain order and ends draining at quiescence.
    pub fn drain_next(&self) -> resident::Drain<W> {
        self.draining
            .drain(|| Some(self.lanes.take::<false>()?.take_value()))
    }
}

impl<W, G, const N: usize> Registry<W, G, resident::Fixed<N>> {
    pub fn lane<const I: usize>(&self) -> handoff::Lane<'_, W, G, resident::Fixed<N>> {
        handoff::Lane {
            registry: self,
            lane: resident::Lane::new::<N, I>(),
        }
    }
}

impl<W, G, L> Registry<W, G, L> {
    pub(super) fn claim(
        &self,
        key: resident::Key,
        slot: &mut Registration<'_, W, G, L>,
    ) -> handoff::Grant<G> {
        match self.lanes.claim(key) {
            None => handoff::Grant::Pending,
            Some(entry) => {
                slot.lease.detach();
                handoff::Grant::Ready(entry.take_grant())
            }
        }
    }

    pub(super) fn cancel(&self, key: resident::Key) -> Cancel<G> {
        match self.lanes.remove(key) {
            resident::Removal::Linked => Cancel::Wake,
            resident::Removal::Parked(entry) => Cancel::Grant(entry.take_grant()),
            resident::Removal::Empty => Cancel::Empty,
        }
    }
}

impl<W, G, L> resident::Target for Registry<W, G, L> {
    type Key = resident::Key;

    fn retire(&self, key: Self::Key) {
        // Only Copy values are linked; parked grants retain their drop obligation.
        if let resident::Removal::Parked(entry) = self.lanes.remove(key) {
            entry.retire_grant();
        }
    }
}

union Payload<W, G> {
    value: mem::ManuallyDrop<W>,
    grant: mem::ManuallyDrop<G>,
}

pub(in crate::depot::queue::resident::handoff) struct Entry<W, G> {
    payload: cell::Cell<mem::MaybeUninit<Payload<W, G>>>,
}

impl<W, G> Default for Entry<W, G> {
    fn default() -> Self {
        Self {
            payload: cell::Cell::new(mem::MaybeUninit::uninit()),
        }
    }
}

impl<W, G> Entry<W, G> {
    pub(in crate::depot::queue::resident::handoff) fn write_value(&self, value: W)
    where
        W: Copy,
    {
        self.payload.set(mem::MaybeUninit::new(Payload {
            value: mem::ManuallyDrop::new(value),
        }));
    }

    pub(in crate::depot::queue::resident::handoff) fn take_value(&self) -> W {
        let payload = self.payload.replace(mem::MaybeUninit::uninit());
        // SAFETY: linked topology proves the value union field is initialized.
        unsafe { mem::ManuallyDrop::into_inner(payload.assume_init().value) }
    }

    pub(in crate::depot::queue::resident::handoff) fn write_grant(&self, grant: G) {
        self.payload.set(mem::MaybeUninit::new(Payload {
            grant: mem::ManuallyDrop::new(grant),
        }));
    }

    pub(in crate::depot::queue::resident::handoff) fn take_grant(&self) -> G {
        let payload = self.payload.replace(mem::MaybeUninit::uninit());
        // SAFETY: marked topology proves the grant union field is initialized.
        unsafe { mem::ManuallyDrop::into_inner(payload.assume_init().grant) }
    }

    pub(in crate::depot::queue::resident::handoff) fn retire_grant(&self) {
        let mut payload = self.payload.replace(mem::MaybeUninit::uninit());
        // SAFETY: marked topology proves the grant union field is initialized.
        unsafe { mem::ManuallyDrop::drop(&mut payload.assume_init_mut().grant) };
    }
}

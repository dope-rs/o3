use std::ptr;

use crate::depot::queue::resident::{self, handoff};

#[derive(Clone, Copy)]
pub struct Lane<'a, W, G, L> {
    pub(super) registry: &'a handoff::Registry<W, G, L>,
    pub(super) lane: resident::Lane,
}

impl<'a, W, G, L> Lane<'a, W, G, L> {
    pub fn try_keep(
        self,
        slot: &mut handoff::Registration<'a, W, G, L>,
        value: W,
    ) -> Result<(), handoff::Reject>
    where
        W: Copy,
    {
        let registry = self.registry;
        let lane = self.lane;
        let index = lane.index().get();
        if registry.is_draining() {
            return Err(handoff::Reject::Draining { index });
        }
        match slot.lease.active() {
            Some((active, key)) => {
                if active.lanes.is_parked(*key) {
                    return Err(handoff::Reject::GrantPending { index });
                }
                if !ptr::eq(active, registry) {
                    return Err(handoff::Reject::Active { index });
                }
                let (next, entry) = active.lanes.relink_back(*key, lane);
                entry.write_value(value);
                *key = next;
            }
            None => {
                let (key, entry) = registry
                    .lanes
                    .try_push(lane, handoff::Reject::Full { index })?;
                entry.write_value(value);
                slot.lease.set(registry, key);
            }
        }
        Ok(())
    }

    pub fn offer_one(self, grant: G) -> handoff::Offer<W, G> {
        if self.registry.is_draining() {
            return handoff::Offer::Draining(grant);
        }
        let Some(entry) = self.registry.lanes.park(self.lane) else {
            return handoff::Offer::Empty(grant);
        };
        let value = entry.take_value();
        entry.write_grant(grant);
        handoff::Offer::Value(value)
    }

    pub fn take_one(self) -> resident::Take<W> {
        let Some(entry) = self.registry.lanes.pop(self.lane) else {
            return resident::Take::Empty;
        };
        resident::Take::Item(entry.take_value())
    }

    pub fn is_empty(self) -> bool {
        self.registry.lanes.is_empty(self.lane)
    }
}

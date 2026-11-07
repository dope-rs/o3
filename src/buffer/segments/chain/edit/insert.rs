use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{
            Chain, Front, Handle, Key,
            chain::edit::{Cause, Growth},
            handles,
            handles::{Descriptor, Occupied},
        },
    },
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

impl<T: buffer::raw::Segment, H: Handle<T>> Chain<T, H> {
    pub fn try_insert(
        &mut self,
        item: segments::Item<T>,
    ) -> Result<Front<'_, T, H>, crate::mem::Rejected<segments::Item<T>, segments::Error>> {
        let error = 'rejected: {
            let (count, len, resident) = self.totals();
            let growth = match Growth::try_new(count, len, resident, &item) {
                Ok(growth) => growth,
                Err(error) => break 'rejected error.into(),
            };
            {
                let nodes = handles::nodes(&self.handle);
                let Some(node) = nodes.vacant() else {
                    break 'rejected Cause::ONE_NODE.into_error();
                };
                match self.key.state() {
                    None => {
                        let claim = node.insert(item);
                        self.key = Key::single(claim.index());
                        // SAFETY: the key above attaches this exact claim slot as the
                        // sequence's sole node before publication.
                        unsafe { claim.publish() };
                    }
                    Some(Occupied::Single(previous)) => {
                        let Some(desc) = handles::descriptors(&self.handle).vacant() else {
                            break 'rejected segments::Error::ChainCapacity(capacity::Shortage {
                                requested: NonZero::<usize>::MIN,
                                available: 0,
                            });
                        };
                        let mut links = ends::Exclusive::empty();
                        let mut live = node.insert(item);
                        // SAFETY: `previous` is the sequence's live single node, `live` is
                        // its sole detached successor, and both belong to the same store.
                        unsafe { links.append_end(live.owner_mut(), previous) };
                        // SAFETY: `links` now holds that live node set and `live` is the
                        // sole fresh claim from the same store, with no successor.
                        unsafe { live.append(&mut links) };
                        let desc = desc.insert(Descriptor {
                            links,
                            count: NonZero::<u32>::MIN.saturating_add(1),
                            len: growth.len,
                            resident_bytes: growth.resident,
                        });
                        self.key = Key::multi(desc.index());
                        // SAFETY: the key above names this descriptor, which now owns the
                        // complete live node set, count, and checked accounting.
                        unsafe { desc.publish() };
                    }
                    Some(Occupied::Multi(index)) => {
                        // SAFETY: the multi key names this live descriptor in the exact
                        // store borrowed exclusively through this chain.
                        let desc =
                            unsafe { &mut *handles::descriptors(&self.handle).value_ptr(index) };
                        // SAFETY: the multi key owns this descriptor's live end-set and
                        // the fresh claim belongs to its exact store, with no successor.
                        unsafe { growth.insert::<false, _>(desc, node.insert(item)) };
                    }
                }
            }
            return Ok(Front { sequence: self });
        };
        Err(crate::mem::Rejected::new(item, error))
    }
}

impl Growth {
    /// # Safety
    /// `desc` owns the exact live partition in the fresh claim's store, and the
    /// claim has no successor. This growth admits that partition plus the claim.
    pub(in crate::buffer::segments::chain) unsafe fn insert<
        const FRONT: bool,
        T: buffer::raw::Segment,
    >(
        self,
        desc: &mut Descriptor,
        claim: slots::Live<segments::Item<T>, &slots::Store<segments::Item<T>>>,
    ) {
        if FRONT {
            unsafe { claim.prepend(&mut desc.links) };
        } else {
            unsafe { claim.append(&mut desc.links) };
        }
        desc.count = self.count;
        desc.len = self.len;
        desc.resident_bytes = self.resident;
    }
}

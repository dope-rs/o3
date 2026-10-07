use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{
            Chain, Handle, Key, Plural,
            chain::edit::{Cause, Growth},
            handles,
            handles::Descriptor,
        },
    },
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

pub(in crate::buffer::segments) struct Front;
pub(in crate::buffer::segments) struct Back;

pub(in crate::buffer::segments) trait Edge: Sized {
    /// # Safety
    /// `links` must exclusively own this edge's live descriptor ends and `claim`
    /// its sole admitted node; both must remain unchanged until publication.
    unsafe fn insert<T>(
        self,
        links: &mut ends::Exclusive<slots::Index>,
        claim: slots::Live<T, &'_ slots::Store<T>>,
    );
}

/// Exact reserved slots, original key borrow, and checked accounting.
pub(in crate::buffer::segments) struct Insert<'a, T: buffer::raw::Segment, E> {
    key: &'a mut Key,
    node: slots::Vacant<segments::Item<T>, &'a slots::Store<segments::Item<T>>>,
    item: segments::Item<T>,
    state: State<'a>,
    edge: E,
}

enum State<'a> {
    Empty,
    Promote {
        previous: slots::Index,
        desc: slots::Vacant<Descriptor, &'a slots::Store<Descriptor>>,
        growth: Growth,
    },
    Multi {
        desc: &'a mut Descriptor,
        growth: Growth,
    },
}

impl<'a, T: buffer::raw::Segment, E: Edge> Insert<'a, T, E> {
    pub(in crate::buffer::segments::chain) fn try_new<H: Handle<T>>(
        sequence: &'a mut Chain<T, H>,
        item: segments::Item<T>,
        edge: E,
    ) -> Result<Self, crate::mem::Rejected<segments::Item<T>, segments::Error>> {
        let growth = match Growth::try_new(sequence, &item) {
            Ok(growth) => growth,
            Err(error) => return Err(crate::mem::Rejected::new(item, error.into())),
        };
        Self::admit(sequence, item, growth, edge)
            .map_err(|(error, item, _)| crate::mem::Rejected::new(item, error.into_error()))
    }

    pub(in crate::buffer::segments::chain::edit) fn admit<H: Handle<T>>(
        sequence: &'a mut Chain<T, H>,
        item: segments::Item<T>,
        growth: Growth,
        edge: E,
    ) -> Result<Self, (Cause, segments::Item<T>, Growth)> {
        let nodes = handles::nodes(&sequence.sequence.handle);
        let Some(node) = nodes.vacant() else {
            return Err((
                Cause::Nodes(capacity::Shortage {
                    requested: NonZero::<usize>::MIN,
                    available: 0,
                }),
                item,
                growth,
            ));
        };
        let state = match sequence.sequence.key.state() {
            handles::State::Empty => State::Empty,
            handles::State::Single(previous) => {
                let Some(desc) = handles::descriptors(&sequence.sequence.handle).vacant() else {
                    return Err((
                        Cause::Descriptors(capacity::Shortage {
                            requested: NonZero::<usize>::MIN,
                            available: 0,
                        }),
                        item,
                        growth,
                    ));
                };
                State::Promote {
                    previous,
                    desc,
                    growth,
                }
            }
            handles::State::Multi(index) => {
                // SAFETY: the multi key names this live descriptor in the exact
                // store borrowed exclusively through the sequence parameter.
                let desc = unsafe {
                    &mut *handles::descriptors(&sequence.sequence.handle).value_ptr(index)
                };
                State::Multi { desc, growth }
            }
        };
        Ok(Self {
            key: &mut sequence.sequence.key,
            node,
            item,
            state,
            edge,
        })
    }

    pub(in crate::buffer::segments) fn commit(self) {
        let Self {
            key,
            node,
            item,
            state,
            edge,
        } = self;
        let claim = node.insert(item);
        match state {
            State::Empty => {
                *key = Key::single(claim.index());
                // SAFETY: the key above attaches this exact claim slot as the
                // sequence's sole node before publication.
                unsafe { claim.publish() };
            }
            State::Promote {
                previous,
                desc,
                growth,
            } => {
                let mut links = ends::Exclusive::empty();
                let mut live = claim;
                // SAFETY: `previous` is the sequence's live single node, `live` is
                // its sole detached successor, and both belong to the same store.
                unsafe { links.append_store(live.owner_mut(), previous) };
                // SAFETY: `links` now holds that live node set and `live` is the
                // sole claim admitted from the same store.
                unsafe { edge.insert(&mut links, live) };
                let desc = desc.insert(Descriptor::new(
                    links,
                    NonZero::<u32>::MIN.saturating_add(1),
                    growth.len,
                    growth.resident,
                ));
                *key = Key::multi(desc.index());
                // SAFETY: the key above names this descriptor, which now owns the
                // complete live node set, count, and checked accounting.
                unsafe { desc.publish() };
            }
            State::Multi { desc, growth } => {
                // SAFETY: the multi key owns this descriptor's live end-set and
                // `claim` is the sole node admitted from its exact store.
                unsafe { edge.insert(&mut desc.links, claim) };
                // SAFETY: the multi state retains a nonzero segment count.
                let count = Plural(unsafe { NonZero::new_unchecked(growth.count.into_raw()) });
                desc.count = count.nonzero();
                desc.len = growth.len;
                desc.resident_bytes = growth.resident;
            }
        }
    }
}

impl Edge for Front {
    unsafe fn insert<T>(
        self,
        links: &mut ends::Exclusive<slots::Index>,
        claim: slots::Live<T, &'_ slots::Store<T>>,
    ) {
        // SAFETY: the caller proved `links` is the live end-set of the claim's
        // exact store and that `claim` is its sole admitted node.
        unsafe { claim.prepend(links) };
    }
}

impl Edge for Back {
    unsafe fn insert<T>(
        self,
        links: &mut ends::Exclusive<slots::Index>,
        claim: slots::Live<T, &'_ slots::Store<T>>,
    ) {
        // SAFETY: the caller proved `links` is the live end-set of the claim's
        // exact store and that `claim` is its sole admitted node.
        unsafe { claim.append(links) };
    }
}

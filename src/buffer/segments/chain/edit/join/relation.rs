use std::{iter, mem, num::NonZero};

use segments::{assembly, assembly::total};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{
            Key,
            chain::edit::{Cause, Growth, join::incoming::Incoming},
            handles,
            handles::{Descriptor, Occupied},
        },
    },
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

type Exclusive<'a, T> = (&'a mut segments::Chain<T>, &'a mut segments::Chain<T>);

pub(in crate::buffer::segments) enum Relation<'a, T> {
    /// Disjoint sequences bounded by one metadata pool.
    Same(Exclusive<'a, T>),
    Other(Exclusive<'a, T>),
}

pub(in crate::buffer::segments) enum Links {
    Singles {
        target: slots::Index,
        source: slots::Index,
    },
    Prepend {
        target: slots::Index,
        source: slots::Index,
    },
    Push {
        target: slots::Index,
        source: slots::Index,
    },
    Splice {
        target: slots::Index,
        source: slots::Index,
    },
}

enum Placement {
    Same(Links),
    Other(Input),
}

pub(in crate::buffer::segments::chain::edit::join) enum Input {
    Single(slots::Index),
    Multi {
        index: slots::Index,
        count: NonZero<u32>,
    },
}

pub(in crate::buffer::segments) enum Rejected {
    Total(total::Error),
    Capacity(Growth, Cause),
}

struct Capacity<T: buffer::raw::Segment> {
    pair: (segments::Many<T>, segments::Many<T>),
    sum: Growth,
    cause: Cause,
}

impl<'a, T: buffer::raw::Segment> Relation<'a, T> {
    pub(in crate::buffer::segments) fn new(pair: Exclusive<'a, T>) -> Self {
        let (target, source) = &pair;
        if target.is_same(source) {
            Self::Same(pair)
        } else {
            Self::Other(pair)
        }
    }

    pub(in crate::buffer::segments) fn try_commit(self) -> Result<(), Rejected> {
        let (mut pair, state) = match self {
            Relation::Same(mut pair) => {
                let (target, source) = &pair;
                let links = match (target.key.state(), source.key.state()) {
                    (_, None) => return Ok(()),
                    (None, _) => {
                        let (target, source) = &mut pair;
                        mem::swap(&mut target.key, &mut source.key);
                        return Ok(());
                    }
                    (Some(Occupied::Single(target)), Some(Occupied::Single(source))) => {
                        Links::Singles { target, source }
                    }
                    (Some(Occupied::Single(target)), Some(Occupied::Multi(source))) => {
                        Links::Prepend { target, source }
                    }
                    (Some(Occupied::Multi(target)), Some(Occupied::Single(source))) => {
                        Links::Push { target, source }
                    }
                    (Some(Occupied::Multi(target)), Some(Occupied::Multi(source))) => {
                        Links::Splice { target, source }
                    }
                };
                (pair, Placement::Same(links))
            }
            Relation::Other(pair) => {
                let (_, source) = &pair;
                let from = match source.key.state() {
                    None => return Ok(()),
                    Some(Occupied::Single(index)) => Input::Single(index),
                    Some(Occupied::Multi(index)) => Input::Multi {
                        index,
                        count: unsafe { handles::descriptors(&source.handle).value(index) }.count,
                    },
                };
                (pair, Placement::Other(from))
            }
        };
        let (target, source) = &pair;
        let (current, len, resident) = target.totals();
        let (added, bytes, retained) = source.totals();
        let count = match &state {
            Placement::Same(_) => current.into_raw().wrapping_add(added.into_raw()),
            Placement::Other(_) => total::Error::count(current, added)
                .map_err(Rejected::Total)?
                .into_raw(),
        };
        let len = total::Error::logical(len, bytes).map_err(Rejected::Total)?;
        let resident = total::Error::resident(resident, retained).map_err(Rejected::Total)?;
        // SAFETY: the source is nonempty. Same-store partitions are disjoint and fit
        // physical node capacity, so their sum cannot wrap; cross-store sums are checked.
        let sum = Growth {
            count: unsafe { NonZero::new_unchecked(count) },
            len,
            resident,
        };
        let result =
            (|| {
                match state {
                    Placement::Same(links) => {
                        let (target, source) = &mut pair;
                        let nodes = handles::nodes(&target.handle);
                        let descs = handles::descriptors(&target.handle);
                        let desc = match links {
                            Links::Singles {
                                target: left,
                                source: right,
                            } => {
                                let claim = descs.vacant().ok_or(Cause::Descriptors(
                                    capacity::Shortage {
                                        requested: NonZero::<usize>::MIN,
                                        available: 0,
                                    },
                                ))?;
                                let mut links = ends::Exclusive::empty();
                                unsafe {
                                    links.append_end(nodes, left);
                                    links.append_end(nodes, right);
                                }
                                let claim = claim.insert(Descriptor {
                                    links,
                                    count: sum.count,
                                    len: sum.len,
                                    resident_bytes: sum.resident,
                                });
                                target.key = Key::multi(claim.index());
                                source.key = Key::EMPTY;
                                unsafe { claim.publish() };
                                return Ok(());
                            }
                            Links::Prepend {
                                target: left,
                                source: right,
                            } => {
                                let desc = unsafe { &mut *descs.value_ptr(right) };
                                unsafe { desc.links.prepend_link(nodes, left) };
                                target.key = Key::multi(right);
                                desc
                            }
                            Links::Push {
                                target: left,
                                source: right,
                            } => {
                                let desc = unsafe { &mut *descs.value_ptr(left) };
                                unsafe { desc.links.append_end(nodes, right) };
                                desc
                            }
                            Links::Splice {
                                target: left,
                                source: right,
                            } => {
                                let (mut from, retired) = unsafe { descs.detached(right) }.take();
                                let desc = unsafe { &mut *retired.release().value_ptr(left) };
                                // SAFETY: Relation::Same retained disjoint chains in this exact store.
                                unsafe { desc.links.append_chain(nodes, &mut from.links) };
                                desc
                            }
                        };
                        desc.count = sum.count;
                        desc.len = sum.len;
                        desc.resident_bytes = sum.resident;
                        source.key = Key::EMPTY;
                        Ok(())
                    }
                    Placement::Other(from) => {
                        let (target, source) = &mut pair;
                        let store = handles::nodes(&target.handle);
                        let table = handles::descriptors(&target.handle);
                        let nodes = slots::Input::try_new(
                            store,
                            Incoming {
                                source,
                                input: from,
                            },
                        )?;
                        let (mut links, output) = match target.key.state() {
                            None => {
                                let output = if sum.count.get() == 1 {
                                    None
                                } else {
                                    let claim = table.vacant().ok_or(Cause::Descriptors(
                                        capacity::Shortage {
                                            requested: NonZero::<usize>::MIN,
                                            available: 0,
                                        },
                                    ))?;
                                    Some(claim)
                                };
                                (ends::Exclusive::empty(), output)
                            }
                            Some(Occupied::Single(index)) => {
                                let claim = table.vacant().ok_or(Cause::Descriptors(
                                    capacity::Shortage {
                                        requested: NonZero::<usize>::MIN,
                                        available: 0,
                                    },
                                ))?;
                                let mut links = ends::Exclusive::empty();
                                unsafe { links.append_end(store, index) };
                                (links, Some(claim))
                            }
                            Some(Occupied::Multi(index)) => {
                                let (desc, claim) = unsafe { table.detached(index) }.take();
                                (desc.links, Some(claim))
                            }
                        };
                        target.key = Key::EMPTY;
                        nodes.start().0.run(|claim, value| {
                            unsafe { claim.insert(value).append(&mut links) };
                        });
                        match output {
                            None => target.key = Key::single(links.head_raw()),
                            Some(claim) => {
                                let claim = claim.insert(Descriptor {
                                    links,
                                    count: sum.count,
                                    len: sum.len,
                                    resident_bytes: sum.resident,
                                });
                                target.key = Key::multi(claim.index());
                                unsafe { claim.publish() };
                            }
                        }
                        Ok(())
                    }
                }
            })();
        match result {
            Ok(()) => Ok(()),
            Err(cause) => Err(Rejected::Capacity(sum, cause)),
        }
    }
}

impl<T: buffer::raw::Segment> TryFrom<(Self, Self)> for segments::Many<T> {
    type Error = crate::mem::Rejected<(Self, Self), assembly::Error>;

    fn try_from((mut first, mut second): (Self, Self)) -> Result<Self, Self::Error> {
        match Relation::new((&mut first.0, &mut second.0)).try_commit() {
            Ok(()) => Ok(first),
            Err(Rejected::Total(cause)) => Err(crate::mem::Rejected::new(
                (first, second),
                assembly::Error::Total(cause),
            )),
            Err(Rejected::Capacity(sum, cause)) => {
                let current = first.0.count();
                let capacity = Capacity {
                    pair: (first, second),
                    sum,
                    cause,
                };
                Ok(segments::Exact::try_reserve(capacity, current)?.collect())
            }
        }
    }
}

impl Rejected {
    pub(in crate::buffer::segments) fn into_error(self) -> segments::Error {
        match self {
            Self::Total(cause) => cause.into(),
            Self::Capacity(_, cause) => cause.into_error(),
        }
    }
}

// SAFETY: both retained Many chains own at least four owners with their checked exact sum.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T> for Capacity<T> {
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(segments::Many<T>, segments::Many<T>), assembly::Error>;

    fn owners(
        self,
    ) -> (
        impl Iterator<Item = segments::Item<T>>,
        impl buffer::raw::Total,
    ) {
        let (first, second) = self.pair;
        let mut first: segments::Chain<T> = first.into();
        let mut second: segments::Chain<T> = second.into();
        let owners =
            iter::from_fn(move || first.pop_item()).chain(iter::from_fn(move || second.pop_item()));
        (owners, self.sum)
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        let (first, second) = self.pair;
        crate::mem::Rejected::new((first, second), self.cause.allocation(error))
    }
}

impl<T: buffer::raw::Segment> buffer::raw::Total for Capacity<T> {
    fn count(&self) -> segments::Count {
        segments::Count::new(self.sum.count.get())
    }
    fn len(&self) -> usize {
        self.sum.len
    }
    fn resident(&self) -> usize {
        self.sum.resident
    }
}

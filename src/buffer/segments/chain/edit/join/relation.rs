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
            handles::Descriptor,
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

enum Output<'a> {
    Single,
    Multi(slots::Vacant<Descriptor, &'a slots::Store<Descriptor>>),
}

pub(in crate::buffer::segments) enum Rejected<P> {
    Total(total::Error),
    Capacity(Capacity<P>),
}

pub(in crate::buffer::segments) struct Capacity<P> {
    pub(in crate::buffer::segments::chain::edit::join) pair: P,
    sum: Growth,
    pub(in crate::buffer::segments::chain::edit::join) cause: Cause,
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

    pub(in crate::buffer::segments) fn try_commit(self) -> Result<(), Rejected<Exclusive<'a, T>>> {
        let (mut pair, state) = match self {
            Relation::Same(mut pair) => {
                let (target, source) = &pair;
                let links = match (target.key.state(), source.key.state()) {
                    (_, handles::State::Empty) => return Ok(()),
                    (handles::State::Empty, _) => {
                        let (target, source) = &mut pair;
                        mem::swap(&mut target.key, &mut source.key);
                        return Ok(());
                    }
                    (handles::State::Single(target), handles::State::Single(source)) => {
                        Links::Singles { target, source }
                    }
                    (handles::State::Single(target), handles::State::Multi(source)) => {
                        Links::Prepend { target, source }
                    }
                    (handles::State::Multi(target), handles::State::Single(source)) => {
                        Links::Push { target, source }
                    }
                    (handles::State::Multi(target), handles::State::Multi(source)) => {
                        Links::Splice { target, source }
                    }
                };
                (pair, Placement::Same(links))
            }
            Relation::Other(pair) => {
                let (_, source) = &pair;
                let from = match source.key.state() {
                    handles::State::Empty => return Ok(()),
                    handles::State::Single(index) => Input::Single(index),
                    handles::State::Multi(index) => Input::Multi {
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
                                let claim = claim.insert(Descriptor::new(
                                    links,
                                    sum.count,
                                    sum.len,
                                    sum.resident,
                                ));
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
                            handles::State::Empty => {
                                let output = if sum.count.get() == 1 {
                                    Output::Single
                                } else {
                                    let claim = table.vacant().ok_or(Cause::Descriptors(
                                        capacity::Shortage {
                                            requested: NonZero::<usize>::MIN,
                                            available: 0,
                                        },
                                    ))?;
                                    Output::Multi(claim)
                                };
                                (ends::Exclusive::empty(), output)
                            }
                            handles::State::Single(index) => {
                                let claim = table.vacant().ok_or(Cause::Descriptors(
                                    capacity::Shortage {
                                        requested: NonZero::<usize>::MIN,
                                        available: 0,
                                    },
                                ))?;
                                let mut links = ends::Exclusive::empty();
                                unsafe { links.append_end(store, index) };
                                (links, Output::Multi(claim))
                            }
                            handles::State::Multi(index) => {
                                let (desc, claim) = unsafe { table.detached(index) }.take();
                                (desc.links, Output::Multi(claim))
                            }
                        };
                        target.key = Key::EMPTY;
                        nodes.start().0.run(|claim, value| {
                            unsafe { claim.insert(value).append(&mut links) };
                        });
                        match output {
                            Output::Single => target.key = Key::single(links.head_raw()),
                            Output::Multi(claim) => {
                                let claim = claim.insert(Descriptor::new(
                                    links,
                                    sum.count,
                                    sum.len,
                                    sum.resident,
                                ));
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
            Err(cause) => Err(Rejected::Capacity(Capacity { pair, sum, cause })),
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
            Err(Rejected::Capacity(Capacity { sum, cause, .. })) => {
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

impl<T> Rejected<(&mut segments::Chain<T>, &mut segments::Chain<T>)> {
    pub(in crate::buffer::segments) fn into_error(self) -> segments::Error {
        match self {
            Self::Total(cause) => segments::Error::Total(cause),
            Self::Capacity(capacity) => capacity.cause.into_error(),
        }
    }
}

// SAFETY: this total owns both unchanged chains and their checked cardinality.
unsafe impl<T: buffer::raw::Segment> buffer::raw::Sequence<T>
    for Capacity<(segments::Many<T>, segments::Many<T>)>
{
    type Output = segments::Many<T>;
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

impl<T: buffer::raw::Segment> buffer::raw::Total
    for Capacity<(segments::Many<T>, segments::Many<T>)>
{
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

use std::{iter, marker::PhantomData, mem, num::NonZero};

use segments::{assembly, assembly::total};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{
            Key,
            chain::{
                Assemble,
                edit::{
                    Cause,
                    join::{Pair, incoming::Incoming},
                },
            },
            handles,
            handles::Descriptor,
        },
    },
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

pub(in crate::buffer::segments) struct Join<T: buffer::raw::Segment, C: Assemble<T>> {
    pub(super) first: segments::Chain<T>,
    pub(super) second: segments::Chain<T>,
    owner: PhantomData<C>,
}

pub(in crate::buffer::segments) struct Committed<P>(P);

pub(in crate::buffer::segments) enum Relation<P> {
    /// Disjoint sequences bounded by one metadata pool.
    Same(P),
    Other(P),
}

pub(in crate::buffer::segments) struct Total<P> {
    pub(in crate::buffer::segments::chain::edit::join) pair: P,
    sum: Sum,
}

pub(in crate::buffer::segments) struct Sum {
    count: NonZero<u32>,
    len: usize,
    resident: usize,
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
    pub(in crate::buffer::segments::chain::edit::join) total: Total<P>,
    pub(in crate::buffer::segments::chain::edit::join) cause: Cause,
}

impl<P: Pair> Relation<P> {
    pub(in crate::buffer::segments) fn new(pair: P) -> Self {
        let (target, source) = pair.as_ref();
        if target.is_same(source) {
            Self::Same(pair)
        } else {
            Self::Other(pair)
        }
    }

    pub(in crate::buffer::segments) fn try_commit(self) -> Result<(), Rejected<P>> {
        let (pair, state) = match self {
            Relation::Same(mut pair) => {
                let (target, source) = pair.as_ref();
                let links = match (target.sequence.key.state(), source.sequence.key.state()) {
                    (_, handles::State::Empty) => return Ok(()),
                    (handles::State::Empty, _) => {
                        let (target, source) = pair.as_mut();
                        mem::swap(&mut target.sequence.key, &mut source.sequence.key);
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
                let (_, source) = pair.as_ref();
                let from = match source.sequence.key.state() {
                    handles::State::Empty => return Ok(()),
                    handles::State::Single(index) => Input::Single(index),
                    handles::State::Multi(index) => Input::Multi {
                        index,
                        count: unsafe {
                            handles::descriptors(&source.sequence.handle).value(index)
                        }
                        .count,
                    },
                };
                (pair, Placement::Other(from))
            }
        };
        let (target, source) = pair.as_ref();
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
        // SAFETY: every prepared state retains a nonempty source.
        let mut total = Total {
            pair,
            sum: Sum {
                count: unsafe { NonZero::new_unchecked(count) },
                len,
                resident,
            },
        };
        let result = (|| {
            match state {
                Placement::Same(links) => {
                    let (target, source) = total.pair.as_mut();
                    let nodes = handles::nodes(&target.sequence.handle);
                    let descs = handles::descriptors(&target.sequence.handle);
                    let desc = match links {
                        Links::Singles {
                            target: left,
                            source: right,
                        } => {
                            let claim =
                                descs
                                    .vacant()
                                    .ok_or(Cause::Descriptors(capacity::Shortage {
                                        requested: NonZero::<usize>::MIN,
                                        available: 0,
                                    }))?;
                            let mut links = ends::Exclusive::empty();
                            unsafe {
                                links.append_store(nodes, left);
                                links.append_store(nodes, right);
                            }
                            let claim = claim.insert(Descriptor::new(
                                links,
                                total.sum.count,
                                total.sum.len,
                                total.sum.resident,
                            ));
                            target.sequence.key = Key::multi(claim.index());
                            source.sequence.key = Key::EMPTY;
                            unsafe { claim.publish() };
                            return Ok(());
                        }
                        Links::Prepend {
                            target: left,
                            source: right,
                        } => {
                            let desc = unsafe { &mut *descs.value_ptr(right) };
                            unsafe { desc.links.prepend_link(nodes, left) };
                            target.sequence.key = Key::multi(right);
                            desc
                        }
                        Links::Push {
                            target: left,
                            source: right,
                        } => {
                            let desc = unsafe { &mut *descs.value_ptr(left) };
                            unsafe { desc.links.append_store(nodes, right) };
                            desc
                        }
                        Links::Splice {
                            target: left,
                            source: right,
                        } => {
                            let (mut from, retired) = unsafe { descs.detached(right) }.take();
                            let desc = unsafe { &mut *retired.release().value_ptr(left) };
                            // SAFETY: Relation::Same retained disjoint chains in this exact store.
                            let pair =
                                unsafe { ends::Pair::new(nodes, &mut desc.links, &mut from.links) };
                            pair.append();
                            desc
                        }
                    };
                    desc.count = total.sum.count;
                    desc.len = total.sum.len;
                    desc.resident_bytes = total.sum.resident;
                    source.sequence.key = Key::EMPTY;
                    Ok(())
                }
                Placement::Other(from) => {
                    let (target, source) = total.pair.as_mut();
                    let store = handles::nodes(&target.sequence.handle);
                    let table = handles::descriptors(&target.sequence.handle);
                    let nodes = slots::Input::try_new(
                        store,
                        Incoming {
                            source,
                            input: from,
                        },
                    )?;
                    let (mut links, output) = match target.sequence.key.state() {
                        handles::State::Empty => {
                            let output = if total.sum.count.get() == 1 {
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
                            let claim =
                                table
                                    .vacant()
                                    .ok_or(Cause::Descriptors(capacity::Shortage {
                                        requested: NonZero::<usize>::MIN,
                                        available: 0,
                                    }))?;
                            let mut links = ends::Exclusive::empty();
                            unsafe { links.append_store(store, index) };
                            (links, Output::Multi(claim))
                        }
                        handles::State::Multi(index) => {
                            let (desc, claim) = unsafe { table.detached(index) }.take();
                            (desc.links, Output::Multi(claim))
                        }
                    };
                    target.sequence.key = Key::EMPTY;
                    nodes.start().0.run(|claim, value| {
                        unsafe { claim.insert(value).append(&mut links) };
                    });
                    match output {
                        Output::Single => target.sequence.key = Key::single(links.head_raw()),
                        Output::Multi(claim) => {
                            let claim = claim.insert(Descriptor::new(
                                links,
                                total.sum.count,
                                total.sum.len,
                                total.sum.resident,
                            ));
                            target.sequence.key = Key::multi(claim.index());
                            unsafe { claim.publish() };
                        }
                    }
                    Ok(())
                }
            }
        })();
        match result {
            Ok(()) => Ok(()),
            Err(cause) => Err(Rejected::Capacity(Capacity { cause, total })),
        }
    }
}

impl<T: buffer::raw::Segment, C: Assemble<T>> Join<T, C> {
    pub(in crate::buffer::segments) fn new(first: C, second: C) -> Self {
        Self {
            first: first.into(),
            second: second.into(),
            owner: PhantomData,
        }
    }

    pub(in crate::buffer::segments) fn try_commit(
        mut self,
    ) -> Result<C, crate::mem::Rejected<(C, C), assembly::Error>>
    where
        Self: Into<(C, C)>,
        Committed<Self>: Into<C>,
    {
        match Relation::new(&mut self).try_commit() {
            Ok(()) => Ok(Committed(self).into()),
            Err(Rejected::Total(cause)) => Err(crate::mem::Rejected::new(
                self.into(),
                assembly::Error::Total(cause),
            )),
            Err(Rejected::Capacity(Capacity {
                total: Total { sum, .. },
                cause,
            })) => {
                let current = self.first.count();
                let capacity = Capacity {
                    total: Total { pair: self, sum },
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
unsafe impl<T: buffer::raw::Segment, C: Assemble<T>> buffer::raw::Sequence<T>
    for Capacity<Join<T, C>>
where
    Join<T, C>: Into<(C, C)>,
{
    type Output = C;
    type Item = segments::Item<T>;
    type Error = crate::mem::Rejected<(C, C), assembly::Error>;

    fn owners(self) -> impl Iterator<Item = segments::Item<T>> + buffer::raw::Total {
        let Join {
            mut first,
            mut second,
            ..
        } = self.total.pair;
        let owners =
            iter::from_fn(move || first.pop_item()).chain(iter::from_fn(move || second.pop_item()));
        buffer::raw::Batch {
            owners,
            total: self.total.sum,
        }
    }

    fn reject(self, error: crate::alloc::Error) -> Self::Error {
        crate::mem::Rejected::new(self.total.pair.into(), self.cause.allocation(error))
    }
}

impl<T: buffer::raw::Segment, C: Assemble<T>> buffer::raw::Total for Capacity<Join<T, C>> {
    fn count(&self) -> segments::Count {
        segments::Count::new(self.total.sum.count.get())
    }
    fn len(&self) -> usize {
        self.total.sum.len
    }
    fn resident(&self) -> usize {
        self.total.sum.resident
    }
}

impl buffer::raw::Total for Sum {
    fn count(&self) -> segments::Count {
        segments::Count::new(self.count.get())
    }
    fn len(&self) -> usize {
        self.len
    }
    fn resident(&self) -> usize {
        self.resident
    }
}

impl<T: buffer::raw::Segment> From<Committed<Join<T, segments::Chain<T>>>> for segments::Chain<T> {
    fn from(val: Committed<Join<T, segments::Chain<T>>>) -> Self {
        val.0.first
    }
}

impl<T: buffer::raw::Segment> From<Committed<Join<T, segments::Many<T>>>> for segments::Many<T> {
    fn from(val: Committed<Join<T, segments::Many<T>>>) -> Self {
        // SAFETY: successful join preserved both original Many inputs in the destination.
        unsafe { segments::Many::new(val.0.first) }
    }
}

impl<T: buffer::raw::Segment> From<Join<T, segments::Chain<T>>>
    for (segments::Chain<T>, segments::Chain<T>)
{
    fn from(val: Join<T, segments::Chain<T>>) -> Self {
        (val.first, val.second)
    }
}

impl<T: buffer::raw::Segment> From<Join<T, segments::Many<T>>>
    for (segments::Many<T>, segments::Many<T>)
{
    fn from(val: Join<T, segments::Many<T>>) -> Self {
        // SAFETY: rejected preflight leaves both original Many inputs unchanged.
        let first = unsafe { segments::Many::new(val.first) };
        // SAFETY: this is the other unchanged input of the same rejected operation.
        let second = unsafe { segments::Many::new(val.second) };
        (first, second)
    }
}

const _: () = {
    use crate::buffer::storage::Shared;
    assert!(
        size_of::<Join<Shared, segments::Many<Shared>>>()
            == 2 * size_of::<segments::Many<Shared>>()
    );
    assert!(
        size_of::<Committed<Join<Shared, segments::Many<Shared>>>>()
            == size_of::<Join<Shared, segments::Many<Shared>>>()
    );
};

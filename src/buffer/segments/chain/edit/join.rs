use std::{iter, mem, num::NonZero};

use segments::{assembly, assembly::total};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{
            Key,
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

type Exclusive<'a, T> = (&'a mut segments::Chain<T>, &'a mut segments::Chain<T>);

pub(in crate::buffer::segments) enum Relation<'a, T> {
    /// Disjoint sequences bounded by one metadata pool.
    Same(Exclusive<'a, T>),
    Other(Exclusive<'a, T>),
}

enum Placement {
    Same(Occupied, Occupied),
    Other(Occupied),
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
            Relation::Same((target, source)) => {
                let Some(right) = source.key.state() else {
                    return Ok(());
                };
                let Some(left) = target.key.state() else {
                    mem::swap(&mut target.key, &mut source.key);
                    return Ok(());
                };
                ((target, source), Placement::Same(left, right))
            }
            Relation::Other(pair) => {
                let (_, source) = &pair;
                let Some(from) = source.key.state() else {
                    return Ok(());
                };
                (pair, Placement::Other(from))
            }
        };
        let (target, source) = &pair;
        let (current, len, resident) = target.totals();
        let (added, bytes, retained) = source.totals();
        let count = match &state {
            Placement::Same(..) => current.into_raw().wrapping_add(added.into_raw()),
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
        let result = (|| {
            match state {
                Placement::Same(left, right) => {
                    let (target, source) = &mut pair;
                    let nodes = handles::nodes(&target.handle);
                    let descs = handles::descriptors(&target.handle);
                    let desc = match (left, right) {
                        (Occupied::Single(left), Occupied::Single(right)) => {
                            let claim = descs
                                .vacant()
                                .ok_or(Cause::Descriptors(capacity::Shortage::ONE))?;
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
                        (Occupied::Single(left), Occupied::Multi(right)) => {
                            let desc = unsafe { &mut *descs.value_ptr(right) };
                            unsafe { desc.links.prepend_link(nodes, left) };
                            target.key = Key::multi(right);
                            desc
                        }
                        (Occupied::Multi(left), Occupied::Single(right)) => {
                            let desc = unsafe { &mut *descs.value_ptr(left) };
                            unsafe { desc.links.append_end(nodes, right) };
                            desc
                        }
                        (Occupied::Multi(left), Occupied::Multi(right)) => {
                            let (from, retired) = unsafe { descs.detached(right) }.take();
                            let desc = unsafe { &mut *retired.release().value_ptr(left) };
                            // SAFETY: the two multi keys own disjoint nonempty chains in this store.
                            unsafe { desc.links.append_chain(nodes, from.links) };
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
                    let reserve =
                        slots::Reserve::try_new(store, capacity::Count::new(added.into_raw()))
                            .map_err(Cause::Nodes)?;
                    let output = match target.key.state() {
                        Some(Occupied::Multi(index)) => {
                            // SAFETY: the exclusive target retains this exact live descriptor.
                            let desc = unsafe { &mut *table.value_ptr(index) };
                            desc.count = sum.count;
                            desc.len = sum.len;
                            desc.resident_bytes = sum.resident;
                            Some(&mut desc.links)
                        }
                        prior if sum.count.get() > 1 => {
                            let claim = table
                                .vacant()
                                .ok_or(Cause::Descriptors(capacity::Shortage::ONE))?;
                            let links = match prior {
                                // SAFETY: this target owns the single node being promoted.
                                Some(Occupied::Single(index)) => unsafe {
                                    ends::Exclusive::from_ends(index, index)
                                },
                                _ => ends::Exclusive::empty(),
                            };
                            let live = claim.insert(Descriptor {
                                links,
                                count: sum.count,
                                len: sum.len,
                                resident_bytes: sum.resident,
                            });
                            let index = live.index();
                            target.key = Key::multi(index);
                            // SAFETY: the target owns the published descriptor and its exclusive links.
                            Some(unsafe { &mut (*live.publish().value_ptr(index)).links })
                        }
                        _ => None,
                    };
                    let input = match from {
                        // SAFETY: the source key owns exactly this single node.
                        Occupied::Single(index) => unsafe {
                            ends::Exclusive::from_ends(index, index)
                        },
                        // SAFETY: the source alone owns this descriptor and transfers its complete chain.
                        Occupied::Multi(index) => {
                            unsafe { handles::descriptors(&source.handle).take::<_, true>(index) }
                                .links
                        }
                    };
                    source.key = Key::EMPTY;
                    // SAFETY: the two pools are distinct. The original key and `added` name the
                    // same nonempty partition, and both capacity admissions are complete.
                    let moved = unsafe { reserve.move_from(handles::nodes(&source.handle), input) };
                    match output {
                        // SAFETY: the move yielded a distinct nonempty partition in this store.
                        Some(links) => unsafe { links.append_chain(store, moved) },
                        None => target.key = Key::single(moved.head_raw()),
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

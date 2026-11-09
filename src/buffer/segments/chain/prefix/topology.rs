use std::num::NonZero;

use handles::{Descriptor, Occupied};

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Key, Kind, Pool, handles},
    },
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

type Claim<'a> = slots::Vacant<Descriptor, &'a slots::Store<Descriptor>>;

pub(in crate::buffer::segments::chain::prefix) struct Body<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) handle: &'a Pool<T>,
    key: &'a mut Key,
    pub(in crate::buffer::segments::chain::prefix) links: ends::Exclusive<slots::Index>,
    pub(in crate::buffer::segments::chain::prefix) len: usize,
    pub(in crate::buffer::segments::chain::prefix) resident: usize,
}

pub(in crate::buffer::segments::chain::prefix) struct Topology<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) body: Body<'a, T>,
    output: Option<(NonZero<u32>, Claim<'a>)>,
}

pub(in crate::buffer::segments::chain::prefix) enum Original<'a, T> {
    Direct(Body<'a, T>),
    Multi(Existing<'a, T>),
}

pub(in crate::buffer::segments::chain::prefix) struct Existing<'a, T> {
    handle: &'a Pool<T>,
    key: &'a mut Key,
    index: slots::Index,
}

enum Source<'a, T> {
    Direct(Body<'a, T>),
    Keep {
        old: Existing<'a, T>,
        count: NonZero<u32>,
    },
    Free(Existing<'a, T>),
}

impl<'a, T: buffer::raw::Segment> Original<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) fn new(
        handle: &'a Pool<T>,
        key: &'a mut Key,
        single: impl FnOnce(slots::Index) -> (usize, usize),
    ) -> Self {
        let nodes = handles::nodes(handle);
        let (links, len, resident) = match key.state() {
            Some(Occupied::Multi(index)) => {
                return Self::Multi(Existing { handle, key, index });
            }
            None => (ends::Exclusive::empty(), 0, 0),
            Some(Occupied::Single(index)) => {
                let (len, resident) = single(index);
                let mut links = ends::Exclusive::empty();
                unsafe { links.append_end(nodes, index) };
                (links, len, resident)
            }
        };
        Self::Direct(Body {
            handle,
            key,
            links,
            len,
            resident,
        })
    }

    pub(in crate::buffer::segments::chain::prefix) fn handle(&self) -> &'a Pool<T> {
        match self {
            Self::Direct(body) => body.handle,
            Self::Multi(old) => old.handle,
        }
    }

    pub(in crate::buffer::segments::chain::prefix) fn total(
        &self,
    ) -> (segments::Count, usize, usize) {
        match self {
            Self::Direct(body) => (
                segments::Count::new(u32::from(!body.links.is_empty())),
                body.len,
                body.resident,
            ),
            Self::Multi(old) => {
                // SAFETY: the original exclusive key retains this live descriptor.
                let desc = unsafe { handles::descriptors(old.handle).value(old.index) };
                (
                    segments::Count::new(desc.count.get()),
                    desc.len,
                    desc.resident_bytes,
                )
            }
        }
    }
}

impl<'a, T> Existing<'a, T> {
    fn take(self) -> (Body<'a, T>, Claim<'a>) {
        let (desc, claim) =
            unsafe { handles::descriptors(self.handle).detached(self.index) }.take();
        *self.key = Key::EMPTY;
        (
            Body {
                handle: self.handle,
                key: self.key,
                links: desc.links,
                len: desc.len,
                resident: desc.resident_bytes,
            },
            claim,
        )
    }
}

impl<'a, T> Source<'a, T> {
    fn new(input: Original<'a, T>, count: segments::Count) -> Self {
        match input {
            Original::Direct(body) => Self::Direct(body),
            Original::Multi(old) => match count.kind() {
                Kind::Empty | Kind::One => Self::Free(old),
                Kind::Many(count) => Self::Keep {
                    old,
                    count: count.nonzero(),
                },
            },
        }
    }

    fn take(self) -> Topology<'a, T> {
        match self {
            Self::Direct(body) => Topology { body, output: None },
            Self::Free(old) => Topology {
                body: old.take().0,
                output: None,
            },
            Self::Keep { old, count } => {
                let (body, claim) = old.take();
                Topology {
                    body,
                    output: Some((count, claim)),
                }
            }
        }
    }
}

impl<'a, T: buffer::raw::Segment> Topology<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) fn try_pair(
        source: Original<'a, T>,
        target: Original<'a, T>,
        remains: segments::Count,
        count: segments::Count,
        different_pool: &bool,
    ) -> Result<(Self, Self), segments::Error> {
        let source = Source::new(source, remains);
        let (body, output, source) = match target {
            Original::Multi(old) => {
                let (body, claim) = old.take();
                // SAFETY: a positive prefix extends the original multi sequence.
                let count = unsafe { NonZero::new_unchecked(count.into_raw()) };
                (body, Some((count, claim)), source)
            }
            Original::Direct(body) => match count.kind() {
                Kind::Empty | Kind::One => (body, None, source),
                Kind::Many(count) => {
                    let count = count.nonzero();
                    match (different_pool, source) {
                        (false, Source::Free(old)) => {
                            let (from, claim) = old.take();
                            return Ok((
                                Self {
                                    body: from,
                                    output: None,
                                },
                                Self {
                                    body,
                                    output: Some((count, claim)),
                                },
                            ));
                        }
                        (_, source) => {
                            let claim = handles::descriptors(body.handle)
                                .vacant()
                                .ok_or(segments::Error::ChainCapacity(capacity::Shortage::ONE))?;
                            (body, Some((count, claim)), source)
                        }
                    }
                }
            },
        };
        Ok((source.take(), Self { body, output }))
    }
}

impl<T> Topology<'_, T> {
    pub(in crate::buffer::segments::chain::prefix) fn restore(self) {
        match self.output {
            None => *self.body.key = Key::from(self.body.links.head_raw()),
            Some((count, claim)) => {
                let live = claim.insert(Descriptor {
                    links: self.body.links,
                    count,
                    len: self.body.len,
                    resident_bytes: self.body.resident,
                });
                *self.body.key = Key::multi(live.index());
                unsafe { live.publish() };
            }
        }
    }
}

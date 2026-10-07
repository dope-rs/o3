use std::{num::NonZero, ops};

use handles::Descriptor;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Key, Kind, Pool, chain::prefix, handles},
    },
    depot::{
        arena::linked::{fifo::ends, slots},
        capacity,
    },
};

pub(in crate::buffer::segments::chain::prefix) struct Body<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) handle: &'a Pool<T>,
    key: &'a mut Key,
    pub(in crate::buffer::segments::chain::prefix) links: ends::Exclusive<slots::Index>,
    pub(in crate::buffer::segments::chain::prefix) len: usize,
    pub(in crate::buffer::segments::chain::prefix) resident: usize,
}

pub(in crate::buffer::segments::chain::prefix) struct Topology<'a, T> {
    body: Body<'a, T>,
    output: Output<'a>,
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

enum Small {
    Empty,
    One,
}

enum Output<'a> {
    Small(Small),
    Many {
        count: NonZero<u32>,
        claim: slots::Vacant<Descriptor, &'a slots::Store<Descriptor>>,
    },
}

enum Source<'a, T> {
    Direct {
        body: Body<'a, T>,
        state: Small,
    },
    Keep {
        old: Existing<'a, T>,
        count: NonZero<u32>,
    },
    Free {
        old: Existing<'a, T>,
        state: Small,
    },
}

impl<'a, T: buffer::raw::Segment> Original<'a, T> {
    pub(in crate::buffer::segments::chain::prefix) fn new(
        handle: &'a Pool<T>,
        key: &'a mut Key,
        single: impl FnOnce(slots::Index) -> (usize, usize),
    ) -> Self {
        let nodes = handles::nodes(handle);
        let (links, len, resident) = match key.state() {
            handles::State::Multi(index) => {
                return Self::Multi(Existing { handle, key, index });
            }
            handles::State::Empty => (ends::Exclusive::empty(), 0, 0),
            handles::State::Single(index) => {
                let (len, resident) = single(index);
                let mut links = ends::Exclusive::empty();
                unsafe { links.append_store(nodes, index) };
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
    fn take(
        self,
    ) -> (
        Body<'a, T>,
        slots::Vacant<Descriptor, &'a slots::Store<Descriptor>>,
    ) {
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
            Original::Direct(body) => Self::Direct {
                body,
                state: if count == segments::Count::EMPTY {
                    Small::Empty
                } else {
                    Small::One
                },
            },
            Original::Multi(old) => match count.kind() {
                Kind::Empty => Self::Free {
                    old,
                    state: Small::Empty,
                },
                Kind::One => Self::Free {
                    old,
                    state: Small::One,
                },
                Kind::Many(count) => Self::Keep {
                    old,
                    count: count.nonzero(),
                },
            },
        }
    }

    fn take(self) -> Topology<'a, T> {
        match self {
            Self::Direct { body, state } => Topology {
                body,
                output: Output::Small(state),
            },
            Self::Free { old, state } => Topology {
                body: old.take().0,
                output: Output::Small(state),
            },
            Self::Keep { old, count } => {
                let (body, claim) = old.take();
                Topology {
                    body,
                    output: Output::Many { count, claim },
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
        relation: &prefix::Relation,
    ) -> Result<(Self, Self), segments::Error> {
        let source = Source::new(source, remains);
        let (body, output, source) = match target {
            Original::Multi(old) => {
                let (body, claim) = old.take();
                // SAFETY: a positive prefix extends the original multi sequence.
                let count = unsafe { NonZero::new_unchecked(count.into_raw()) };
                (body, Output::Many { count, claim }, source)
            }
            Original::Direct(body) => match count.kind() {
                Kind::Empty => (body, Output::Small(Small::Empty), source),
                Kind::One => (body, Output::Small(Small::One), source),
                Kind::Many(count) => {
                    let count = count.nonzero();
                    match (relation, source) {
                        (prefix::Relation::Same, Source::Free { old, state }) => {
                            let (from, claim) = old.take();
                            return Ok((
                                Self {
                                    body: from,
                                    output: Output::Small(state),
                                },
                                Self {
                                    body,
                                    output: Output::Many { count, claim },
                                },
                            ));
                        }
                        (_, source) => {
                            let claim = handles::descriptors(body.handle).vacant().ok_or(
                                segments::Error::ChainCapacity(capacity::Shortage {
                                    requested: NonZero::<usize>::MIN,
                                    available: 0,
                                }),
                            )?;
                            (body, Output::Many { count, claim }, source)
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
            Output::Small(Small::Empty) => *self.body.key = Key::EMPTY,
            Output::Small(Small::One) => *self.body.key = Key::single(self.body.links.head_raw()),
            Output::Many { count, claim } => {
                let live = claim.insert(Descriptor::new(
                    self.body.links,
                    count,
                    self.body.len,
                    self.body.resident,
                ));
                *self.body.key = Key::multi(live.index());
                unsafe { live.publish() };
            }
        }
    }
}

impl<'a, T> ops::Deref for Topology<'a, T> {
    type Target = Body<'a, T>;
    fn deref(&self) -> &Self::Target {
        &self.body
    }
}

impl<T> ops::DerefMut for Topology<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.body
    }
}

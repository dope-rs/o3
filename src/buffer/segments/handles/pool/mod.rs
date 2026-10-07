use crate::{buffer, depot::arena::linked::slots, mem};
pub(in crate::buffer::segments) mod exact;

use std::num::NonZero;

use crate::{
    buffer::{
        hold::{self, scope},
        resident, segments,
        segments::{
            Handle, Header, handles,
            handles::{Descriptor, Key, Parts},
            queue,
        },
    },
    depot::capacity,
    mem::rc,
};

#[derive(Debug)]
#[repr(transparent)]
pub struct Pool<T> {
    owner: mem::raw::Owner<Header<T>>,
}

impl<T> Pool<T> {
    pub fn try_new(plan: segments::Plan) -> Result<Self, crate::alloc::Error> {
        let layout = Parts::<T>::try_new(plan)?;
        Self::allocate(layout)
    }

    pub(in crate::buffer::segments) fn try_reserve(
        current: segments::Count,
        required: segments::Count,
    ) -> Result<Self, crate::alloc::Error> {
        let layout = Parts::<T>::try_reserve(current, required)?;
        Self::allocate(layout)
    }

    fn allocate(layout: Parts<T>) -> Result<Self, crate::alloc::Error> {
        let heap = layout.bundle.layout();
        let fields = layout.bundle.try_allocate()?;
        let pointer = fields.prior().prior().root();
        let nodes = fields.prior().tail();
        let desc = fields.tail();
        let header = Header {
            allocation: heap,
            refs: mem::raw::Count::one(),
            // SAFETY: this pointer and capacity describe the segment tail.
            nodes: unsafe { slots::Store::new(nodes, layout.nodes) },
            // SAFETY: this pointer and capacity describe the descriptor tail.
            descriptors: unsafe { slots::Store::new(desc, layout.descriptors) },
        };
        // SAFETY: the header prefix is aligned, uniquely owned, and uninitialized.
        unsafe { pointer.write(header) };
        Ok(Self {
            // SAFETY: the initialized allocation transfers its initial edge.
            owner: unsafe { mem::raw::Owner::from_raw(pointer) },
        })
    }

    pub fn capacity(&self) -> segments::Plan {
        segments::Plan {
            segments: self.header().nodes.region.capacity().len(),
            chains: self.header().descriptors.region.capacity().len(),
        }
    }

    pub fn available(&self) -> segments::Plan {
        segments::Plan {
            segments: self.header().nodes.available(),
            chains: self.header().descriptors.available(),
        }
    }
}

impl<T> Handle<T> for Pool<T> {
    fn header(&self) -> &segments::Header<T> {
        self.owner.get()
    }
}

impl<T> Pool<T> {
    pub(in crate::buffer::segments) fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            owner: self.owner.try_clone()?,
        })
    }
}

enum State<'a> {
    Empty,
    One,
    Many {
        count: segments::Plural,
        claim: slots::Vacant<Descriptor, &'a slots::Store<Descriptor>>,
    },
}

impl<T> Pool<T> {
    /// Returns whether every segment and chain slot is available.
    pub fn is_idle(&self) -> bool {
        self.available() == self.capacity()
    }

    pub fn into_chain(self) -> segments::Chain<T>
    where
        T: buffer::raw::Segment,
    {
        segments::Chain::new(self)
    }

    /// Creates a detached chain owning one non-atomic pool edge.
    pub fn try_chain(&self) -> Result<segments::Chain<T>, rc::Error>
    where
        T: buffer::raw::Segment,
    {
        Ok(segments::Chain::new(self.try_clone()?))
    }

    /// Retains owners transactionally within the available metadata capacity.
    pub(in crate::buffer::segments) fn try_build<E: From<segments::Error>>(
        &self,
        plan: impl queue::Build<T, E>,
    ) -> Result<segments::Chain<T>, E>
    where
        T: buffer::raw::Segment,
    {
        let count = segments::Count::new(plan.count().raw());
        let nodes = handles::nodes(self);
        let descs = handles::descriptors(self);
        let reserve = slots::Input::try_new(nodes, plan)?;
        let state = match count.kind() {
            segments::Kind::Empty => State::Empty,
            segments::Kind::One => State::One,
            segments::Kind::Many(count) => State::Many {
                count,
                claim: descs.vacant().ok_or(segments::Error::ChainCapacity(
                    capacity::Shortage {
                        requested: NonZero::<usize>::MIN,
                        available: 0,
                    },
                ))?,
            },
        };
        let handle = self.try_clone().map_err(segments::Error::Retain)?;
        let key = {
            let mut writer = exact::Writer::new(nodes);
            let plan = reserve.start().0.try_run(|claim, owner| {
                unsafe { writer.push_claim(claim, owner) };
            })?;
            let links = writer.into_links();
            match state {
                State::Empty => Key::EMPTY,
                State::One => Key::single(links.head_raw()),
                State::Many { count, claim } => {
                    let live = claim.insert(Descriptor::new(
                        links,
                        count.nonzero(),
                        plan.len(),
                        plan.resident(),
                    ));
                    let key = Key::multi(live.index());
                    unsafe { live.publish() };
                    key
                }
            }
        };
        let mut sequence = segments::Chain::new(handle);
        sequence.key = key;
        Ok(sequence)
    }

    /// Creates a chain whose lifetime is bounded by this pool borrow.
    pub fn scoped(&self) -> segments::Chain<T, &segments::Header<T>>
    where
        T: buffer::raw::Segment,
    {
        segments::Chain::new(self.header())
    }
}

impl<'d> Pool<resident::Shared<'d>> {
    /// Reserves one segment before retaining the exact source-bound range.
    pub fn try_retain<R, E>(
        &self,
        source: scope::Bound<'_, 'd, R>,
    ) -> Result<segments::Chain<resident::Shared<'d>>, E>
    where
        R: hold::raw::Retain<'d>,
        E: From<segments::Error> + From<hold::Error>,
    {
        if source.is_empty() {
            return Ok(self.try_chain().map_err(segments::Error::Retain)?);
        }
        let node = handles::nodes(self)
            .vacant()
            .ok_or(segments::Error::SegmentCapacity(capacity::Shortage {
                requested: NonZero::<usize>::MIN,
                available: 0,
            }))?;
        let handle = self.try_clone().map_err(segments::Error::Retain)?;
        let owner = source.try_into()?;
        let mut chain = segments::Chain::new(handle);
        // SAFETY: the admitted source is nonempty and Retain preserves its exact extent.
        let item = unsafe { segments::Item::assume(owner) };
        let live = node.insert(item);
        chain.key = Key::single(live.index());
        // SAFETY: this chain's retained pool edge owns the exact inserted node.
        unsafe { live.publish() };
        Ok(chain)
    }
}

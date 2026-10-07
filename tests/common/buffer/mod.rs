use std::{cell::Cell, collections, marker, ops};

use o3::{
    buffer::{
        self,
        bytes::scope,
        resident::{
            self,
            staging::{self, reserve},
        },
        segments::{self, scatter::Source as _},
        storage::{self, range},
        write::{
            Sink as _,
            consume::{self, Consume},
        },
    },
    mem::{self, rc},
};

pub mod domain;
pub mod pool;
pub mod provided;
pub mod scatter;

pub struct Selected<'id> {
    range: ops::Range<usize>,
    brand: marker::PhantomData<*mut &'id ()>,
}

impl<'id> Selected<'id> {
    pub fn try_new(
        source: &scope::Owner<'id, resident::Shared<'_>>,
        range: ops::Range<usize>,
    ) -> Result<Self, range::Error> {
        source.view().try_get(range.clone())?;
        Ok(Self {
            range,
            brand: marker::PhantomData,
        })
    }
}

// SAFETY: admission binds this exact range to the concrete immutable Shared
// source. The invariant identity admits only unchanged views and full clones.
unsafe impl<'id> scope::raw::Select<'id> for Selected<'id> {
    fn bounds(self) -> ops::Range<usize> {
        self.range
    }
}

#[derive(PartialEq, Eq)]
pub enum Reject {
    Requested,
}

pub struct Redirect {
    pub source: [u8; 3],
    pub target: storage::Shared,
}

impl Consume for Redirect {
    fn remaining(&self) -> usize {
        self.source.len()
    }

    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        self.target.try_consume(amount)
    }

    fn consume_max(&mut self, max: usize) -> usize {
        self.target.consume_max(max)
    }
}

pub trait Bytes {
    fn try_bytes(&self) -> Result<Vec<u8>, collections::TryReserveError>;
}

impl<T: buffer::raw::Segment> Bytes for segments::Chain<T> {
    fn try_bytes(&self) -> Result<Vec<u8>, collections::TryReserveError> {
        use o3::buffer::raw::Scatter as _;

        let mut output = Vec::new();
        output.try_reserve_exact(self.len())?;
        for chunk in self.chunks().take_bytes(usize::MAX) {
            output.extend_from_slice(chunk.bytes());
        }
        Ok(output)
    }
}

#[derive(Debug)]
pub enum Error {
    Allocation(storage::AllocError),
    Empty(storage::Shared),
    References(rc::Error),
    Insert(mem::Rejected<segments::Item<storage::Shared>, segments::Error>),
    Shape(segments::Shape<storage::Shared>),
}

pub fn try_item(bytes: &[u8]) -> Result<segments::Item<storage::Shared>, Error> {
    let owner = storage::Shared::try_copy(bytes).map_err(Error::Allocation)?;
    match segments::Item::new(owner) {
        ops::ControlFlow::Continue(item) => Ok(item),
        ops::ControlFlow::Break(owner) => Err(Error::Empty(owner)),
    }
}

pub fn try_many(
    pool: &segments::Pool<storage::Shared>,
) -> Result<segments::Many<storage::Shared>, Error> {
    let mut chain = pool.try_chain().map_err(Error::References)?;
    for bytes in [b"first".as_slice(), b"second"] {
        chain.try_insert(try_item(bytes)?).map_err(Error::Insert)?;
    }
    match chain.into_shape() {
        segments::Shape::Many(many) => Ok(many),
        shape => Err(Error::Shape(shape)),
    }
}

pub trait Fill<'a> {
    fn try_fill(
        &mut self,
        values: &[&'a [u8]],
    ) -> Result<(), mem::Rejected<resident::Shared<'a>, segments::Error>>;
}

impl<'a> Fill<'a> for segments::Chain<resident::Shared<'a>> {
    fn try_fill(
        &mut self,
        values: &[&'a [u8]],
    ) -> Result<(), mem::Rejected<resident::Shared<'a>, segments::Error>> {
        for value in values {
            self.try_push(resident::Shared::from(*value))?;
        }
        Ok(())
    }
}

pub struct Counted {
    pub bytes: [u8; 3],
    pub reads: Cell<Option<usize>>,
}

impl buffer::Source for Counted {
    type View<'input>
        = &'input [u8]
    where
        Self: 'input;

    fn remaining(&self) -> usize {
        self.bytes.len()
    }
}

impl buffer::Cursor for Counted {
    fn try_get<'input>(
        &'input self,
        range: ops::Range<usize>,
    ) -> Result<Self::View<'input>, range::Error> {
        self.as_ref().try_get(range)
    }
}

impl AsRef<[u8]> for Counted {
    fn as_ref(&self) -> &[u8] {
        self.reads
            .set(self.reads.get().and_then(|count| count.checked_add(1)));
        &self.bytes
    }
}

/// Local staging geometry and initialized bytes, independent of execution scope.
pub struct Local(pub staging::Layout);

impl Local {
    pub fn try_new() -> Result<Self, staging::Error> {
        let plan = staging::Plan::new(1, 8)?;
        staging::Layout::new(1, plan, 0, 12).map(Self)
    }

    pub fn try_buf<'d>(
        &self,
        budget: &resident::Budget<'d>,
    ) -> Result<staging::Buf<'d>, reserve::Error> {
        let pool = staging::Pool::try_new(budget, self.0).map_err(reserve::Error::Resident)?;
        let mut buf = staging::Buf::try_from(&pool)
            .map_err(resident::AllocError::Retain)
            .map_err(reserve::Error::Resident)?;
        buf.try_reserve(4)?;
        buf.write_slice(b"abcd")?;
        Ok(buf)
    }
}

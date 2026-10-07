//! Persistent lane proofs with exclusive mutation governed by one region token.

use std::num::NonZero;

use tagged::lane;

use crate::{
    base::cell::region,
    depot::{
        capacity,
        queue::{indexed::lanes::runtime, tagged},
    },
};

/// Immutable queue identity whose mutable topology belongs to one region.
pub struct Fifo<
    'id,
    T,
    C: crate::depot::raw::Count = capacity::Count,
    L: crate::depot::raw::Count = capacity::Count,
> {
    queue: region::Value<region::Identity<'id>, runtime::Fifo<T, C, L>>,
    lanes: L,
}

/// A checked endpoint retaining its exact queue owner and region brand.
pub struct Head<
    'a,
    'id,
    T,
    C: crate::depot::raw::Count = capacity::Count,
    L: crate::depot::raw::Count = capacity::Count,
> {
    owner: &'a Fifo<'id, T, C, L>,
    endpoint: NonZero<u32>,
}

impl<'id, T, C: crate::depot::raw::Count, L: crate::depot::raw::Count> Fifo<'id, T, C, L> {
    pub fn try_from(
        capacity: C,
        lanes: L,
        token: &region::Token<region::Identity<'id>>,
    ) -> Result<Self, tagged::Error<C, L>> {
        let queue = runtime::Fifo::try_from(capacity, lanes)?;
        Ok(Self {
            lanes: queue.lanes(),
            queue: token.bind(queue),
        })
    }

    pub fn head(&self, index: usize) -> Result<Head<'_, 'id, T, C, L>, lane::Error<L>> {
        let lanes = self.lanes;
        if index >= lanes.count().get() {
            return Err(lane::Error { lane: index, lanes });
        }
        // SAFETY: the immutable count came from this queue's validated allocation.
        let endpoint = unsafe { NonZero::new_unchecked(u32::MAX - index as u32) };
        Ok(Head {
            owner: self,
            endpoint,
        })
    }
}

impl<'a, 'id, T, C: crate::depot::raw::Count, L: crate::depot::raw::Count> Head<'a, 'id, T, C, L> {
    pub const fn index(&self) -> usize {
        (u32::MAX - self.endpoint.get()) as usize
    }

    pub fn borrow<'b>(
        &self,
        token: &'b mut region::Token<region::Identity<'id>>,
    ) -> tagged::Endpoint<&'b mut runtime::Fifo<T, C, L>>
    where
        'a: 'b,
    {
        let queue = token.borrow_mut(&self.owner.queue);
        // SAFETY: owner never exposes replacement of its queue or allocation shape.
        unsafe { tagged::Endpoint::new(queue, self.endpoint) }
    }

    pub fn take(
        &self,
        token: &mut region::Token<region::Identity<'id>>,
        index: usize,
        expected: T,
    ) -> Option<T>
    where
        T: Eq,
    {
        token.borrow_mut(&self.owner.queue).take(index, expected)
    }
}

const _: () = {
    assert!(size_of::<Fifo<'static, ()>>() == 4 * size_of::<usize>());
    assert!(size_of::<Head<'static, 'static, ()>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Option<Head<'static, 'static, ()>>>() == size_of::<Head<'static, 'static, ()>>()
    );
    assert!(
        size_of::<Fifo<'static, (), NonZero<u32>, NonZero<u32>>>()
            == size_of::<Fifo<'static, ()>>()
    );
    assert!(
        size_of::<Option<Head<'static, 'static, (), NonZero<u32>, NonZero<u32>>>>()
            == size_of::<Head<'static, 'static, ()>>()
    );
};

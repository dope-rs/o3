use std::marker;

use depot::{capacity, map};
use map::priority;

use crate::{alloc, depot};

/// A capacity validated against the exact priority-map layout.
pub struct Plan<K, P, V> {
    table: map::raw::Shape<u32>,
    shape: Tag<K, P, V>,
}

impl<K, P: Ord, V> Plan<K, P, V> {
    pub fn new(capacity: usize) -> Result<Self, map::Capacity> {
        let plan = map::Plan::new(capacity).map_err(map::Capacity::Domain)?;
        Self::try_plan(plan).map_err(|cause| map::Capacity::Layout {
            requested: capacity,
            cause,
        })
    }

    fn try_plan(plan: map::Plan) -> Result<Self, alloc::Overflow> {
        let table = map::raw::Layout::<u32>::try_plan(plan)?;
        let shape = table.shape();
        alloc::raw::Recipe::try_build(Self::recipe(table, plan.capacity().get()))?;
        Ok(Self {
            table: shape,
            shape: marker::PhantomData,
        })
    }

    pub(in crate::depot::map::priority) fn into_bundle(
        self,
    ) -> (alloc::raw::Bundle<Fields<K, P, V>>, usize) {
        let table = self.table.layout();
        let buckets = table.buckets();
        // SAFETY: this value-bound plan admitted this exact field sequence.
        let bundle =
            unsafe { alloc::raw::Recipe::build(Self::recipe(table, self.capacity().get())) };
        (bundle, buckets)
    }

    fn recipe(
        table: map::raw::Layout<u32>,
        capacity: usize,
    ) -> impl alloc::raw::Recipe<Fields = Fields<K, P, V>> {
        use alloc::raw::Recipe as _;

        alloc::raw::Recipe::tail::<priority::Node<K, V, P>>(table, capacity).tail::<u32>(capacity)
    }
}

impl<K, P, V> Plan<K, P, V> {
    /// Returns the capacity admitted by this plan's exact layout.
    pub const fn capacity(self) -> capacity::Count {
        self.table.capacity()
    }
}

impl<K, P, V> Clone for Plan<K, P, V> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K, P, V> Copy for Plan<K, P, V> {}

type Tag<K, P, V> = marker::PhantomData<fn() -> (K, P, V)>;

type Fields<K, P, V> = alloc::raw::Tail<
    alloc::raw::Tail<
        <map::raw::Layout<u32> as alloc::raw::Recipe>::Fields,
        priority::Node<K, V, P>,
    >,
    u32,
>;

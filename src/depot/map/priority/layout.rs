use std::marker;

use map::priority;

use crate::{alloc, depot::map};

type Fields<K, V, P> = alloc::raw::Tail<
    alloc::raw::Tail<
        <map::raw::Layout<u32> as alloc::raw::Recipe>::Fields,
        priority::Node<K, V, P>,
    >,
    u32,
>;

pub(in crate::depot::map::priority) struct Layout<K, V, P> {
    pub(in crate::depot::map::priority) bundle: alloc::raw::Bundle<Fields<K, V, P>>,
    pub(in crate::depot::map::priority) buckets: usize,
    pub(in crate::depot::map::priority) plan: priority::Plan<K, P, V>,
}

impl<K, V, P: Ord> Layout<K, V, P> {
    pub(in crate::depot::map::priority) fn try_plan(
        plan: map::Plan,
    ) -> Result<Self, crate::alloc::Overflow> {
        let capacity = plan.capacity();
        let table = map::raw::Layout::<u32>::try_plan(plan)?;
        let buckets = table.buckets();
        let shape = table.shape();
        let bundle = alloc::raw::Recipe::try_build(Self::recipe(table, capacity.get()))?;
        Ok(Self {
            bundle,
            buckets,
            plan: priority::Plan {
                table: shape,
                shape: marker::PhantomData,
            },
        })
    }

    pub(in crate::depot::map::priority) fn from_plan(plan: priority::Plan<K, P, V>) -> Self {
        let capacity = plan.capacity().get();
        let table = plan.table.layout();
        let buckets = table.buckets();
        // SAFETY: the value-bound plan proves this exact extension.
        let bundle = unsafe { alloc::raw::Recipe::build(Self::recipe(table, capacity)) };
        Self {
            bundle,
            buckets,
            plan,
        }
    }

    fn recipe(
        table: map::raw::Layout<u32>,
        capacity: usize,
    ) -> impl alloc::raw::Recipe<Fields = Fields<K, V, P>> {
        use alloc::raw::Recipe as _;

        alloc::raw::Recipe::tail::<priority::Node<K, V, P>>(table, capacity).tail::<u32>(capacity)
    }
}

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Exact, Many, assembly, chain::edit, handles},
    },
};

impl<T: buffer::raw::Segment> Many<T> {
    #[inline(always)]
    pub(in crate::buffer::segments::chain) fn try_insert<const FRONT: bool>(
        self,
        item: segments::Item<T>,
    ) -> Result<Self, crate::mem::Rejected<(Self, segments::Item<T>), assembly::Error>> {
        let mut chain = self;
        let (current, growth) = 'full: {
            let multi = chain.edit();
            let nodes = handles::nodes(&multi.sequence.handle);
            // SAFETY: Multi exclusively owns this live descriptor, disjoint from nodes.
            let desc = unsafe {
                &mut *handles::descriptors(&multi.sequence.handle).value_ptr(multi.index)
            };
            let current = segments::Count::new(desc.count.get());
            let growth = match edit::Growth::try_new(current, desc.len, desc.resident_bytes, &item)
            {
                Ok(growth) => growth,
                Err(error) => {
                    return Err(crate::mem::Rejected::new(
                        (chain, item),
                        assembly::Error::Total(error),
                    ));
                }
            };
            let Some(node) = nodes.vacant() else {
                break 'full (current, growth);
            };
            // SAFETY: this unchanged descriptor and fresh claim belong to the same
            // store partition, and growth checked this exact insertion before claiming.
            unsafe { growth.insert::<FRONT, _>(desc, node.insert(item)) };
            return Ok(chain);
        };
        if FRONT {
            Ok(Exact::try_reserve((item, chain, growth), current)?.collect())
        } else {
            Ok(Exact::try_reserve((chain, item, growth), current)?.collect())
        }
    }
}

use std::array;

use crate::depot::queue::{indexed::slab, links};
const END: u32 = links::Index::END;

/// Admitted capacity retained by the allocation's exclusive borrow.
pub(in crate::depot) struct Reserved<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    const M: usize,
    C: crate::depot::raw::Count,
> {
    core: &'a mut slab::Slab<T, Tag, MAX, H, N, C>,
}

impl<
    'a,
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    const M: usize,
    C: crate::depot::raw::Count,
> Reserved<'a, T, Tag, MAX, H, N, M, C>
{
    pub(in crate::depot) fn new(core: &'a mut slab::Slab<T, Tag, MAX, H, N, C>) -> Option<Self> {
        (M <= core.available()).then_some(Self { core })
    }

    pub(in crate::depot) fn into_parts(
        self,
    ) -> (&'a mut slab::Slab<T, Tag, MAX, H, N, C>, [links::Index; M]) {
        let core = self.core;
        let free = core.available();
        let slots = array::from_fn(|_| {
            let state = core.block.state();
            // SAFETY: the exclusive borrow preserves all M admitted vacancies.
            let index = if state.free == END {
                let index = state.initialized;
                unsafe {
                    core.block.metadata.add(index as usize).as_ptr().write(
                        crate::depot::raw::IndexedMeta {
                            generation: core.block.generation.min(),
                            hook: H::new(links::Index::new_raw(index), None),
                        },
                    );
                    core.block.state_mut().initialized += 1;
                }
                index
            } else {
                let index = state.free;
                let meta = unsafe { core.block.metadata.add(index as usize).as_mut() };
                let next = unsafe { meta.hook.next() };
                meta.hook = H::new(unsafe { links::Index::new_raw(index) }, None);
                unsafe { core.block.state_mut() }.free = next;
                index
            };
            unsafe { links::Index::new_raw(index) }
        });
        if M != 0 {
            unsafe { core.block.state_mut() }.available = (free - M) as u32;
        }
        (core, slots)
    }
}

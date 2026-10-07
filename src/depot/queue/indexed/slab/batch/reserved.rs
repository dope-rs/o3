use std::{array, mem, ptr};

use slab::phase;

use crate::depot::queue::{indexed::slab, links};
const END: u32 = links::Index::END;

/// Distinct reserved positions retained by their exact exclusive allocation.
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
    pub(in crate::depot) core: &'a mut slab::Slab<T, Tag, MAX, H, N, C>,
    pub(in crate::depot) slots: [links::Index; M],
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
        let free = core.available();
        if M > free {
            return None;
        }
        let slots = array::from_fn(|_| {
            let state = core.block.state();
            // SAFETY: admission reserves M distinct available positions in this owner.
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
        Some(Self { core, slots })
    }

    pub(in crate::depot) fn into_parts(
        self,
    ) -> (&'a mut slab::Slab<T, Tag, MAX, H, N, C>, [links::Index; M]) {
        let this = mem::ManuallyDrop::new(self);
        // SAFETY: the complete reservation transfers its owner and positions once.
        (unsafe { ptr::read(&this.core) }, this.slots)
    }
}

impl<
    T,
    Tag,
    const MAX: u32,
    H: crate::depot::raw::IndexedHook,
    const N: usize,
    const M: usize,
    C: crate::depot::raw::Count,
> Drop for Reserved<'_, T, Tag, MAX, H, N, M, C>
{
    fn drop(&mut self) {
        for &index in &self.slots {
            slab::Slot {
                core: self.core,
                index,
                phase: phase::Metadata,
            }
            .release::<false>();
        }
    }
}

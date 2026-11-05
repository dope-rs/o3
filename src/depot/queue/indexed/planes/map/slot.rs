use std::marker;

use crate::depot::queue::{
    indexed::planes::{map, phase},
    links,
};

/// Exclusive membership authority borrowing only disjoint hooks of one installed record.
pub struct Slot<'a, K, V, const N: usize, P = ()> {
    pub(in crate::depot::queue::indexed::planes::map) core: &'a map::Core<K, V, N>,
    pub(in crate::depot::queue::indexed::planes::map) index: links::Index,
    phase: marker::PhantomData<P>,
}

impl<'a, K, V, const N: usize, P> Slot<'a, K, V, N, P> {
    pub(in crate::depot::queue::indexed::planes::map) fn new(
        core: &'a map::Core<K, V, N>,
        index: links::Index,
    ) -> Self {
        Self {
            core,
            index,
            phase: marker::PhantomData,
        }
    }
    pub(in crate::depot::queue::indexed::planes::map) fn into_phase<Q>(
        self,
    ) -> Slot<'a, K, V, N, Q> {
        Slot::new(self.core, self.index)
    }
}

impl<K, V, const N: usize> Slot<'_, K, V, N> {
    pub(in crate::depot::queue::indexed::planes::map) fn is_linked<const P: usize>(&self) -> bool {
        const { assert!(P > 0 && P < N) };
        self.index
            .is_linked(&unsafe { self.core.node(self.index) }.hooks[P])
    }
}

impl<'a, K, V, const N: usize, const P: usize> Slot<'a, K, V, N, phase::Linked<P>> {
    pub fn unlink(self) -> Slot<'a, K, V, N, phase::Vacant<P>> {
        let hooks = map::Hooks {
            core: self.core,
            plane: P,
        };
        // SAFETY: this phase retains exclusive hook authority and its linked membership in P.
        unsafe { self.core.queues[P].unlink(&hooks, self.index) }.idle();
        self.into_phase()
    }
}

impl<'a, K, V, const N: usize, const P: usize> Slot<'a, K, V, N, phase::Vacant<P>> {
    pub fn link_back(self) -> Slot<'a, K, V, N, phase::Linked<P>> {
        let hooks = map::Hooks {
            core: self.core,
            plane: P,
        };
        // SAFETY: this phase retains exclusive hook authority and its vacancy in P.
        unsafe { self.core.queues[P].link_back(&hooks, self.index) };
        self.into_phase()
    }
}

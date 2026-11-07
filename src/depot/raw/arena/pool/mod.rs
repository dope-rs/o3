use crate::depot::arena::pool::shared;

mod mutable;
mod policy;

pub(in crate::depot) use mutable::PoolMutable;
pub(in crate::depot) use policy::PoolPolicy;

// SAFETY: every resident slot owns exactly this reference domain while active.
unsafe impl<T> crate::mem::raw::Edge for shared::Resident<T> {
    fn count(&self) -> &crate::mem::raw::Count {
        &self.references
    }
}

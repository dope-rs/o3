use heap::{Live, keyed, resident};
pub(in crate::depot) use hole::Hole;
pub(in crate::depot) use position::Position;
pub(in crate::depot) use reindex::Reindex;
pub(in crate::depot) use store::Store;

use crate::depot::{heap, index};

mod hole;
mod position;
mod reindex;
mod store;

unsafe impl<'a, K: crate::depot::raw::Address, P: Ord, C, I: index::Position>
    crate::depot::raw::Project<K> for (&'a mut keyed::Min<K, P, I>, C)
{
    type Output = Live<(&'a mut keyed::Min<K, P, I>, C), K>;

    unsafe fn project(self, key: K) -> Self::Output {
        unsafe { crate::depot::raw::Proof::new((self, key)) }
    }
}

unsafe impl<'a, K: crate::depot::raw::Address, P: Ord, R: Copy, C, I: index::Position>
    crate::depot::raw::Project<K> for (&'a mut resident::Min<K, P, R, I>, C)
{
    type Output = Live<(&'a mut resident::Min<K, P, R, I>, C), K>;

    unsafe fn project(self, key: K) -> Self::Output {
        unsafe { crate::depot::raw::Proof::new((self, key)) }
    }
}

/// # Safety
/// The key belongs to this owner's fixed domain, whose residents are initialized.
pub(in crate::depot::heap) unsafe fn resident<
    K: crate::depot::raw::Address,
    P: Ord,
    R: Copy,
    I: index::Position,
>(
    owner: &resident::Min<K, P, R, I>,
    key: K,
) -> &R {
    unsafe { owner.storage.values().get_unchecked(key.index()) }
}

/// # Safety
/// The key belongs to this owner's fixed domain; the exclusive loan retains its resident.
pub(in crate::depot::heap) unsafe fn resident_mut<
    K: crate::depot::raw::Address,
    P: Ord,
    R: Copy,
    I: index::Position,
>(
    owner: &mut resident::Min<K, P, R, I>,
    key: K,
) -> &mut R {
    unsafe { owner.storage.values_mut().get_unchecked_mut(key.index()) }
}

mod queue;
pub(in crate::depot) use queue::Queue;

mod source;

use crate::depot::{
    array, capacity,
    map::priority,
    queue::{
        indexed::{exact, fifo},
        links,
    },
};

// SAFETY: the iterator owns its exact initialized suffix within one u32-sized array domain.
unsafe impl<T, const N: usize> crate::depot::raw::Exact for array::IntoIter<T, N> {
    type Output = ();

    fn remaining(&self) -> capacity::Count {
        capacity::Count::new(self.len() as u32)
    }

    unsafe fn finish(self) {
        drop(self);
    }
}

// SAFETY: the exclusive loan retains the owner's exact suffix until traversal finishes.
unsafe impl<T, const N: usize> crate::depot::raw::Exact for &mut array::IntoIter<T, N> {
    type Output = ();

    fn remaining(&self) -> capacity::Count {
        crate::depot::raw::Exact::remaining(&**self)
    }

    unsafe fn finish(self) {}
}

unsafe impl<'a, T: Copy + Eq, I: crate::depot::raw::Compact> crate::depot::raw::Project<I>
    for &'a mut fifo::List<T, I, exact::Policy>
{
    type Output = exact::Entry<'a, T, I>;

    unsafe fn project(self, index: I) -> Self::Output {
        exact::Entry::from_source(unsafe { crate::depot::raw::Proof::new(((self, ()), index)) }).0
    }
}

// SAFETY: the original owner and initialized physical node remain live until projection.
// This proof cannot survive removal or transfer to another map.
unsafe impl<'a, K, P: Ord, V, H> crate::depot::raw::Project<links::Index>
    for &'a mut priority::Min<K, P, V, H>
{
    type Output = priority::Occupied<'a, K, P, V>;

    unsafe fn project(self, index: links::Index) -> Self::Output {
        // SAFETY: Project supplies this exact owner's live node for the entire output loan.
        priority::Min::entry_at(unsafe { crate::depot::raw::Proof::new((self, index)) })
    }
}

// SAFETY: Index represents every u32 below its sentinel with an exact scalar round trip.
unsafe impl crate::depot::raw::Compact for links::Index {
    const MAX: u32 = u32::MAX - 1;

    fn into_raw(self) -> u32 {
        self.raw()
    }

    unsafe fn from_raw(raw: u32) -> Self {
        // SAFETY: Compact bounds raw below the absent sentinel.
        unsafe { Self::new_raw(raw) }
    }
}

// SAFETY: a u32 count's largest represented index is below the absent sentinel.
unsafe impl crate::depot::raw::Count<links::Index> for capacity::Count {
    fn count(self) -> capacity::Count {
        self
    }
}

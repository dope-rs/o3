use std::num;

use crate::buffer::pool::{self, owners};

mod backing;
pub use backing::Backing;

// SAFETY: Plain pool allocations contain no resident account table.
unsafe impl Backing for owners::Plain {
    type Link = ();
}

// SAFETY: Fixed admission binds both dimensions to this plain allocation.
unsafe impl<const SLOTS: usize, const CAPACITY: usize> Backing
    for owners::Plain<pool::Fixed<SLOTS, CAPACITY>>
{
    type Link = ();

    const SLOTS: Option<u32> = Some(SLOTS as u32);
    const CAPACITY: Option<num::NonZero<u32>> = num::NonZero::new(CAPACITY as u32);
}

// SAFETY: Width admission binds this capacity to every plain allocation it issues.
unsafe impl<const CAPACITY: usize> Backing for owners::Plain<pool::Width<CAPACITY>> {
    type Link = ();

    const CAPACITY: Option<num::NonZero<u32>> = num::NonZero::new(CAPACITY as u32);
}
// SAFETY: Resident preserves the lifetime-bound form of an accounted pool.
unsafe impl<'d> Backing for owners::Resident<'d> {
    type Link = std::cell::Cell<u32>;

    unsafe fn release<S>(core: owners::Ptr<Self, S>) {
        // SAFETY: the terminal resident edge retains the exact table lifetime.
        unsafe { core.uncharge() };
        // SAFETY: the charge is returned before its terminal allocation is freed.
        unsafe { core.deallocate() };
    }
}

// SAFETY: Charged retains one root debit until every slot and the pool have ended.
unsafe impl Backing for owners::Charged<'_> {
    type Link = ();

    unsafe fn release<S>(core: owners::Ptr<Self, S>) {
        // SAFETY: the caller transfers the terminal charged allocation edge.
        unsafe { core.destroy() };
    }
}

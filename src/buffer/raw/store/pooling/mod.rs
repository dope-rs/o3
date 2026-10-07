use crate::buffer::pool::owners;

mod backing;
pub(in crate::buffer) use backing::Backing;

// SAFETY: Plain pool allocations contain no resident account table.
unsafe impl<L> Backing for owners::Plain<L> {
    unsafe fn release<S>(core: owners::Ptr<Self, S>) {
        // SAFETY: the caller transfers the terminal plain allocation edge.
        unsafe { core.deallocate() };
    }
}
// SAFETY: Resident preserves the lifetime-bound form of an accounted pool.
unsafe impl<'d> Backing for owners::Resident<'d> {
    unsafe fn release<S>(core: owners::Ptr<Self, S>) {
        // SAFETY: the terminal resident edge retains the exact table lifetime.
        unsafe { core.uncharge() };
        // SAFETY: the charge is returned before its terminal allocation is freed.
        unsafe { core.deallocate() };
    }
}

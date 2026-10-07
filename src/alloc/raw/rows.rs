use std::mem;

use crate::alloc;

/// Equal-length planes with compile-time row bounds.
/// # Safety
/// Every plane has exactly `len()` elements.
pub unsafe trait Rows: alloc::raw::Shape {
    const ROOT_MAX: usize = match mem::size_of::<Self::Root>() {
        0 => usize::MAX,
        size => isize::MAX as usize / size,
    };

    const MAX: usize = {
        let root = mem::align_of::<Self::Root>();
        let tail = mem::align_of::<Self::Tail>();
        let align = if root > tail { root } else { tail };
        let bound = (isize::MAX as usize) & !(align - 1);
        match mem::size_of::<Self::Root>() + mem::size_of::<Self::Tail>() {
            0 => usize::MAX,
            size => bound / size,
        }
    };

    const EXTRA_MAX: usize = {
        let mut low = 0;
        let mut high = Self::MAX;
        while low < high {
            let span = high - low;
            let count = low + span.div_ceil(2);
            if alloc::raw::Bundle::bounds::<Self>(count, count).fits() {
                low = count;
            } else {
                high = count - 1;
            }
        }
        low
    };
}

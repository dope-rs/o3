#[derive(Debug)]
/// Rejected compact byte-range plan.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Error<N = u32> {
    /// The range count exceeds `u32::MAX`.
    Count {
        actual: usize,
    },
    Order {
        index: u32,
        start: N,
        end: N,
    },
    Bounds {
        index: u32,
        start: N,
        end: N,
        len: usize,
    },
    Length {
        expected: usize,
        actual: usize,
    },
}

const _: () = {
    assert!(size_of::<Error>() <= 4 * size_of::<usize>());
    assert!(size_of::<Error<u16>>() <= 3 * size_of::<usize>());
};

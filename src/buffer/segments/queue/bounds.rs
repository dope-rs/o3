/// Rejected byte span with the exact cursor bounds observed at the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub offset: usize,
    pub amount: usize,
    pub floor: usize,
    pub ceiling: usize,
}

impl Bounds {
    pub(super) const fn new(offset: usize, amount: usize, floor: usize, ceiling: usize) -> Self {
        Self {
            offset,
            amount,
            floor,
            ceiling,
        }
    }
}

const _: () = assert!(size_of::<Bounds>() == 4 * size_of::<usize>());

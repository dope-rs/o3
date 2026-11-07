/// Rejected byte span with the exact cursor bounds observed at the operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub offset: usize,
    pub amount: usize,
    pub floor: usize,
    pub ceiling: usize,
}

const _: () = assert!(size_of::<Bounds>() == 4 * size_of::<usize>());

const _: () = assert!(
    usize::BITS >= 64,
    "o3 requires a 64-bit target: capacities are u32 and index/layout math assumes usize >= u32"
);

pub mod marker;

pub mod buffer;
pub mod cell;
pub mod collections;
pub mod mem;

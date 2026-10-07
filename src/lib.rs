#![doc = include_str!("../README.md")]

pub mod alloc;
pub mod base;
pub mod buffer;
pub mod depot;
pub mod mem;

const _: () = assert!(
    usize::BITS >= 64,
    "o3 requires a 64-bit target: capacities are u32 and index/layout math assumes usize >= u32"
);

#![doc = include_str!("compile_fail.md")]

const _: () = assert!(
    usize::BITS >= 64,
    "o3 requires a 64-bit target: capacities are u32 and index/layout math assumes usize >= u32"
);

pub mod buffer;
pub mod cell;
pub mod collections;
pub mod mem;
pub mod num;
pub mod permit;
pub mod queue;

use std::marker;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ThreadBound(marker::PhantomData<*mut ()>);

impl ThreadBound {
    pub const NEW: Self = Self(marker::PhantomData);
}

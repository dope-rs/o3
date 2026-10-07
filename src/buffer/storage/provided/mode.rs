use std::num;

#[derive(Clone, Copy)]
pub(super) enum Mode {
    Fixed(num::NonZero<u32>),
    Exact,
}

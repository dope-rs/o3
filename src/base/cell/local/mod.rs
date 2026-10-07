//! Closure-scoped capabilities that prevent region-owned references escaping.

mod access;
mod read;

pub use access::Access;
pub use read::Read;

const _: () = {
    use std::mem;

    use crate::base::cell::region::Identity;

    assert!(mem::size_of::<Access<'static, Identity<'static>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Read<'static, Identity<'static>>>() == mem::size_of::<usize>());
};

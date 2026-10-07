use std::{cell, marker};

mod account;
mod live;
pub(crate) use account::Account;
pub(crate) use live::Live;

/// One structural lane's protected reserve and current holdings.
pub struct State<const N: usize = 1, Tag = (), W: crate::mem::raw::Word = usize> {
    pub(in crate::mem::raw) reserve: [W; N],
    pub(in crate::mem::raw) held: cell::Cell<[W; N]>,
    pub(in crate::mem::raw) domain: crate::mem::raw::Domain<Tag>,
}

impl<const N: usize, Tag, W: crate::mem::raw::Word> State<N, Tag, W> {
    /// Creates an empty lane before binding it to its exact shared account.
    pub const fn new(reserve: [W; N]) -> Self {
        const { assert!(N != 0) };
        Self {
            reserve,
            held: cell::Cell::new([W::ZERO; N]),
            domain: marker::PhantomData,
        }
    }
}

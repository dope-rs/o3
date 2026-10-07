use crate::depot::{
    capacity, index,
    queue::{indexed::fifo::staged, links},
};

pub trait Sealed {}

impl Sealed for capacity::Physical {}
impl<P> Sealed for index::Initialized<P> {}
impl<P> Sealed for index::Unborrowed<P> {}

impl Sealed for staged::Exclusive {}

/// Closed numeric domains with built-in operations.
pub trait Numeric {}

impl Numeric for u16 {}
impl Numeric for u32 {}

impl Sealed for capacity::Domain<'_> {}

impl Sealed for links::Vacant {}

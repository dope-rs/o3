use std::mem;

#[derive(Debug)]
/// A priority-entry probe found no remaining map capacity.
#[derive(PartialEq, Eq)]
pub struct Full;

const _: () = {
    assert!(mem::size_of::<Full>() == 0);
    assert!(mem::align_of::<Full>() == 1);
    assert!(mem::size_of::<Option<Full>>() == 1);
};

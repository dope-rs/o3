/// An index outside the source owner's bitmap domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Error {
    pub index: usize,
    pub capacity: usize,
}

const _: () = assert!(size_of::<Error>() == 2 * size_of::<usize>());

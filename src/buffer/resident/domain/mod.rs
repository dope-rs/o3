pub(in crate::buffer::resident) mod budget;
pub(in crate::buffer::resident) mod errors;
pub(in crate::buffer::resident) mod reuse;

/// One stable resident-accounting counter reused across generative regions.
#[derive(Debug)]
pub struct Domain {
    capacity: usize,
    resident: crate::mem::raw::Account<true>,
}

impl Default for Domain {
    fn default() -> Self {
        Self::new()
    }
}

impl Domain {
    pub const fn new() -> Self {
        Self {
            capacity: 0,
            resident: crate::mem::raw::Account::new(0),
        }
    }
}

const _: () = assert!(size_of::<Domain>() == size_of::<usize>() * 2);

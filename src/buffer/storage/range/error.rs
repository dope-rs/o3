use std::ops;

/// Rejected visible byte range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    start: usize,
    end: usize,
    len: usize,
}

impl Error {
    pub const fn new(range: ops::Range<usize>, len: usize) -> Self {
        Self {
            start: range.start,
            end: range.end,
            len,
        }
    }

    pub const fn start(&self) -> usize {
        self.start
    }

    pub const fn end(&self) -> usize {
        self.end
    }

    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub const fn len(&self) -> usize {
        self.len
    }
}

const _: () = assert!(size_of::<Error>() == 3 * size_of::<usize>());

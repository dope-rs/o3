/// Exact rejection of a direct-table vacancy request.
#[derive(Debug, PartialEq, Eq)]

pub enum Error<K: crate::depot::raw::Key> {
    Outside {
        key: K,
        index: usize,
        capacity: usize,
    },
    Occupied {
        key: K,

        stored: K,
    },
}

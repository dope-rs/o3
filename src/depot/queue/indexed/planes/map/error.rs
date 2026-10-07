/// Rejected first admission, retaining the proposed key and observed extent.
#[derive(Debug)]
pub enum Error<K> {
    Bounds {
        key: K,
        index: usize,
        capacity: usize,
    },
    Occupied {
        key: K,
        stored: K,
        index: usize,
    },
    Key {
        key: K,
        stored: K,
        index: usize,
    },
    Full {
        key: K,
        capacity: usize,
    },
}

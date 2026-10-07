pub struct Stored<K: crate::depot::raw::Address, P> {
    pub(in crate::depot) identity: K,
    pub(in crate::depot) priority: P,
}

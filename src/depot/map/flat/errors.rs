#[derive(Debug, PartialEq, Eq)]
pub enum Insert<V> {
    Full(V),
    Occupied(V),
}

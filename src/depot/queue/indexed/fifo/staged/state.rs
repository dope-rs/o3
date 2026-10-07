/// Borrowed state of one indexed staged slot.
pub enum State<'queue, Key> {
    Vacant,
    Queued { key: &'queue Key },
    Parked { key: &'queue Key },
}

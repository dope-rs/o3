#[derive(Clone, Copy)]
pub(in crate::depot) enum Phase {
    Vacant,
    Queued,
    Parked,
}

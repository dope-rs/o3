use crate::depot::arena::linked::slots;

/// Exact result of reserving one slot for a borrowed lane.
pub enum Vacancy<'a, T> {
    Ready(slots::Vacant<T, slots::Bound<'a, T>>),
    Exhausted,
}

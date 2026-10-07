use crate::buffer::resident;

/// Exact ownership scope observed while borrowing one resident owner.
#[must_use]
pub enum Scope<'a, 'd: 'a> {
    Independent,
    Bound(&'a resident::Shared<'d>),
}

const _: () = assert!(size_of::<Scope<'static, 'static>>() == size_of::<usize>());

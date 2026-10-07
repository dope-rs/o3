use crate::depot::arena::linked;

pub trait Sealed<T> {}

impl<T, S: linked::Source<T>> Sealed<T> for &S {}

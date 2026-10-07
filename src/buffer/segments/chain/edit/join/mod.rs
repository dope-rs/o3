use relation::Input;
pub(in crate::buffer::segments) use relation::{Join, Relation};

use crate::{
    buffer,
    buffer::segments::{Chain, Pool, chain::Assemble},
};

mod incoming;
mod relation;

/// Exclusive borrows of two retained chains for one admitted mutation.
pub(in crate::buffer::segments) trait Pair {
    type Owner: buffer::raw::Segment;
    fn as_ref(&self) -> Shared<'_, Self::Owner>;
    fn as_mut(&mut self) -> Exclusive<'_, Self::Owner>;
}

impl<T: buffer::raw::Segment, C: Assemble<T>> Pair for &mut Join<T, C> {
    type Owner = T;

    fn as_ref(&self) -> (&Chain<T, Pool<T>>, &Chain<T, Pool<T>>) {
        (&self.first, &self.second)
    }

    fn as_mut(&mut self) -> (&mut Chain<T, Pool<T>>, &mut Chain<T, Pool<T>>) {
        (&mut self.first, &mut self.second)
    }
}

impl<T: buffer::raw::Segment> Pair for (&mut Chain<T, Pool<T>>, &mut Chain<T, Pool<T>>) {
    type Owner = T;

    fn as_ref(&self) -> (&Chain<T, Pool<T>>, &Chain<T, Pool<T>>) {
        (self.0, self.1)
    }

    fn as_mut(&mut self) -> (&mut Chain<T, Pool<T>>, &mut Chain<T, Pool<T>>) {
        (self.0, self.1)
    }
}

type Shared<'a, T> = (&'a Chain<T, Pool<T>>, &'a Chain<T, Pool<T>>);

type Exclusive<'a, T> = (&'a mut Chain<T, Pool<T>>, &'a mut Chain<T, Pool<T>>);

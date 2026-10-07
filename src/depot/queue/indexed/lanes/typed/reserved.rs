use crate::depot::queue::indexed::lanes::typed;

pub trait Reserved {}

impl<K: crate::depot::raw::Key, C, D, I, const N: usize> Reserved
    for typed::Vacant<'_, K, C, D, I, N>
{
}

impl<K: crate::depot::raw::Key, C, D, I, P: typed::Phase<C, D, I>, const N: usize> Reserved
    for typed::Withdrawn<'_, K, C, D, I, P, N>
{
}

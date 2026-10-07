use crate::depot::index::{
    self, bitmap,
    bitmap::entry::{Removed, Row},
    set,
};

/// A bounded position retaining its original shared or exclusive source.
pub struct Entry<S, Tag = (), Raw = usize, const N: usize = 1, P = (), K = ()> {
    pub(in crate::depot) row: Row<set::View<S, Tag, Raw, N, K>, P>,
}

impl<S: Copy, Tag, Raw, const N: usize> Clone for Entry<S, Tag, Raw, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S: Copy, Tag, Raw, const N: usize> Copy for Entry<S, Tag, Raw, N> {}

impl<S, Tag, Raw: crate::depot::raw::Word, const N: usize, K> Entry<S, Tag, Raw, N, set::Id<N>, K>
where
    S: set::Storage<Tag, Raw, N, K>,
{
    pub(in crate::depot::index::set) fn new(
        removed: Row<set::View<S, Tag, Raw, N, K>, set::Id<N>, Removed>,
    ) -> Self {
        let row = removed.with_phase();
        set::NonEmpty::advance(&row);
        Self { row }
    }
    pub fn restore(self) {
        self.row.update::<true>(self.row.plane());
    }
}

impl<S, Tag, Raw: crate::depot::raw::Word, const N: usize, P, K> Entry<S, Tag, Raw, N, P, K>
where
    S: set::Storage<Tag, Raw, N, K>,
{
    pub fn index(&self) -> index::Ordinal<Tag, Raw> {
        let raw = unsafe { <Raw as crate::depot::raw::Word>::from_offset(self.row.index()) };
        index::Ordinal::new(raw)
    }
}

impl<Tag, Raw: crate::depot::raw::Word, const N: usize>
    Entry<&set::State<Tag, Raw, N>, Tag, Raw, N>
{
    pub fn set(self, plane: set::Id<N>) {
        self.row.update::<true>(plane);
    }
    pub fn clear(self, plane: set::Id<N>) {
        self.row.update::<false>(plane);
    }
}
impl<Tag, Raw: crate::depot::raw::Word, const N: usize>
    Entry<&set::State<Tag, Raw, N, set::Disjoint>, Tag, Raw, N, set::Id<N>>
{
    pub fn insert(self, target: set::Id<N>) {
        self.row.update::<true>(target);
    }
}
impl<'a, Tag, Raw, const N: usize>
    Entry<&'a set::State<Tag, Raw, N>, Tag, Raw, N, set::Id<N>, set::Planar>
{
    pub fn into_entry(self) -> Entry<&'a set::State<Tag, Raw, N>, Tag, Raw, N> {
        Entry {
            // SAFETY: project the same source owner with its original lifetime and domain.
            row: unsafe { self.row.map(|view| set::View::new(view.source)) },
        }
    }
}
impl<
    S: crate::depot::raw::SetSource<Tag, Raw, N>,
    Tag,
    Raw: crate::depot::raw::Word,
    const N: usize,
> Entry<S, Tag, Raw, N, set::Id<N>, set::Planar>
{
    pub fn take_front(source: S, plane: set::Id<N>) -> Option<Self> {
        let start = set::NonEmpty::new(source.planes())?.cursor(plane).get();
        let removed = bitmap::pop_from(set::View::new(source), plane, start)?;
        Some(Self::new(removed))
    }
    pub fn set(self, target: set::Id<N>) {
        self.row.update::<true>(target);
    }
}

const _: () = {
    assert!(
        size_of::<Entry<&'static set::State<(), u16, 5>, (), u16, 5>>() == 2 * size_of::<usize>()
    );
    assert!(
        size_of::<Entry<&'static set::State<(), u16, 2, set::Disjoint>, (), u16, 2, set::Id<2>>>()
            == 2 * size_of::<usize>()
    );
    assert!(
        size_of::<Entry<&'static mut set::State<(), u16, 2>, (), u16, 2, set::Id<2>, set::Planar>>(
        ) == 2 * size_of::<usize>()
    );
};

use crate::depot::index::{self, bitmap, set};

pub struct Plane<'a, Tag = (), Raw = usize, const N: usize = 1> {
    state: &'a set::State<Tag, Raw, N>,
    id: set::Id<N>,
}

impl<Tag, Raw, const N: usize> Clone for Plane<'_, Tag, Raw, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<Tag, Raw, const N: usize> Copy for Plane<'_, Tag, Raw, N> {}

impl<'a, Tag, Raw: crate::depot::raw::Word, const N: usize> Plane<'a, Tag, Raw, N> {
    pub(in crate::depot::index::set) const fn new(
        state: &'a set::State<Tag, Raw, N>,
        id: set::Id<N>,
    ) -> Self {
        Self { state, id }
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty(self.id)
    }

    /// Finds the first occupied position at or after `start`, without wrapping or mutation.
    pub fn find(
        self,
        start: index::Ordinal<Tag, Raw>,
    ) -> Option<set::Entry<&'a set::State<Tag, Raw, N>, Tag, Raw, N>> {
        let row = bitmap::Access::find(set::View::new(self.state), self.id, start.offset())?;
        Some(set::Entry { row })
    }

    pub fn pop_from(self, start: index::Ordinal<Tag, Raw>) -> Option<Removed<'a, Tag, Raw, N>> {
        let removed =
            bitmap::Access::pop_from(set::View::new(self.state), self.id, start.offset())?;
        Some(set::Entry::new(removed))
    }
}

/// Removed membership retains the original shared plane for restoration.
type Removed<'a, Tag, Raw, const N: usize> =
    set::Entry<&'a set::State<Tag, Raw, N>, Tag, Raw, N, set::Id<N>, set::Planar>;

use crate::depot::queue::{indexed::fifo::staged, links};

/// One represented position retaining its exact interior table owner.
pub struct Member<'a, K, Q, P, I: crate::depot::raw::Compact = u32> {
    owner: &'a staged::Table<K, Q, P, I>,
    index: links::Index,
}

impl<'a, K, Q, P, I: crate::depot::raw::Compact> Member<'a, K, Q, P, I> {
    pub fn get(owner: &'a staged::Table<K, Q, P, I>, index: I) -> Option<Self> {
        let index = index.into_raw() as usize;
        if index >= owner.nodes.len() {
            return None;
        }
        Some(Self {
            owner,
            index: unsafe { links::Index::new_usize(index) },
        })
    }

    pub(in crate::depot) fn into_parts(self) -> (&'a staged::Table<K, Q, P, I>, links::Index) {
        (self.owner, self.index)
    }

    /// Reports whether the original table has any queued member.
    pub fn has_queued(&self) -> bool {
        self.owner.nodes.state().head.get().is_some()
    }
}

const _: () = {
    assert!(size_of::<Member<'static, (), (), ()>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Member<'static, (), (), ()>>>() == 2 * size_of::<usize>());
};

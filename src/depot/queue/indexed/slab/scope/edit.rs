use crate::depot::queue::indexed::slab::{Key, Scope};

/// One insertion and its distinct pre-existing parent in the same live scope.
pub struct Edit<'id, Tag> {
    pub(super) child: Key<'id, Tag>,
    pub(super) parent: Option<Key<'id, Tag>>,
}

impl<'id, Tag> Edit<'id, Tag> {
    pub fn get<'a, T, const MAX: u32, C: crate::depot::raw::Count>(
        self,
        scope: &'a mut Scope<'_, 'id, T, Tag, MAX, C>,
    ) -> (Option<&'a mut T>, &'a mut T) {
        let values = unsafe { scope.core.block.values() };
        // SAFETY: the invariant scope brand retains both initialized slots;
        // the parent existed before the child vacancy was reserved, so they
        // are distinct and stay live until this insertion-only scope ends.
        let child = unsafe {
            values
                .add(self.child.index.get())
                .as_mut()
                .assume_init_mut()
        };
        let parent = self
            .parent
            .map(|parent| unsafe { values.add(parent.index.get()).as_mut().assume_init_mut() });
        (parent, child)
    }
}

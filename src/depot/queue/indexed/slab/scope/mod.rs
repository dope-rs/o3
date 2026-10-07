use key::Key;

use crate::depot::queue::{indexed::slab::phase, links};
pub(in crate::depot) mod key;
use edit::Edit;
pub(in crate::depot) mod edit;

use std::{marker, mem};

use slab::slot;

use crate::depot::{capacity, queue::indexed::slab};

/// Keeps every bound slot live; cancellation removes only new insertions.
pub struct Scope<
    'a,
    'id,
    T,
    Tag,
    const MAX: u32 = { u32::MAX },
    C: crate::depot::raw::Count = capacity::Count,
> {
    core: &'a mut slab::Slab<T, Tag, MAX, links::Hook, 0, C>,
    added: Option<(links::Index, links::Index)>,
    brand: marker::PhantomData<fn(&'id mut Tag) -> &'id mut Tag>,
}

impl<'a, T, Tag, const MAX: u32, C: crate::depot::raw::Count> Scope<'a, '_, T, Tag, MAX, C> {
    pub fn with<R>(
        source: &'a mut slab::Slab<T, Tag, MAX, links::Hook, 0, C>,
        run: impl for<'id> FnOnce(Scope<'a, 'id, T, Tag, MAX, C>) -> R,
    ) -> R {
        run(Scope {
            core: source,
            added: None,
            brand: marker::PhantomData,
        })
    }
}

impl<'id, T, Tag, const MAX: u32, C: crate::depot::raw::Count> Scope<'_, 'id, T, Tag, MAX, C> {
    pub fn bind(
        &self,
        handle: slab::Handle<Tag, MAX>,
    ) -> Result<Key<'id, Tag>, slab::Error<Tag, MAX, C>> {
        let resolved = handle.resolve(&*self.core)?;
        Ok(Key {
            index: *resolved.key(),
            brand: marker::PhantomData,
        })
    }

    pub fn get(&self, key: Key<'id, Tag>) -> &T {
        // SAFETY: the invariant brand identifies this allocation; no slot can retire in the scope.
        unsafe { self.core.block.pair(key.index) }.1
    }

    pub fn get_mut(&mut self, key: Key<'id, Tag>) -> &mut T {
        slab::Slot {
            core: self.core,
            index: key.index,
            phase: phase::Occupied,
        }
        .into_mut()
    }

    pub fn key(&self, key: Key<'id, Tag>) -> slab::Handle<Tag, MAX> {
        // SAFETY: binding and insertion are the only Key constructors, and retain initialized metadata.
        unsafe { self.core.block.pair(key.index) }.0
    }

    pub fn try_insert(&mut self, value: T) -> Result<Key<'id, Tag>, crate::mem::Rejected<T, C>> {
        let Some(slot) = self.core.vacant() else {
            return Err(crate::mem::Rejected::new(value, self.core.capacity()));
        };
        let index = slot.insert(value).index;
        Ok(self.inserted(index))
    }

    /// Reserves first, then moves an existing parent's value into the insertion.
    pub fn try_splice(
        &mut self,
        parent: Option<Key<'id, Tag>>,
        build: impl FnOnce(Option<&mut T>, slab::Handle<Tag, MAX>) -> T,
    ) -> Result<Edit<'id, Tag>, slab::Full<C>> {
        let Some(slot) = self.core.vacant() else {
            return Err(slab::Full {
                capacity: self.core.capacity(),
            });
        };
        // SAFETY: this scope's branded parent is initialized and distinct
        // from the reserved vacancy, which has no Key until the splice commits.
        let splice = match parent {
            Some(parent) => {
                slot::Splice::Parent(unsafe { crate::depot::raw::Proof::new((slot, parent.index)) })
            }
            None => slot::Splice::Root(slot),
        };
        let index = splice.insert(build).index;
        Ok(Edit {
            child: self.inserted(index),
            parent,
        })
    }

    fn inserted(&mut self, index: links::Index) -> Key<'id, Tag> {
        let first = match self.added {
            Some((first, _)) => first,
            None => index,
        };
        self.added = Some((first, index));
        Key {
            index,
            brand: marker::PhantomData,
        }
    }

    pub fn commit(mut self) {
        self.added = None;
    }
}

impl<T, Tag, const MAX: u32, C: crate::depot::raw::Count> Drop for Scope<'_, '_, T, Tag, MAX, C> {
    fn drop(&mut self) {
        while let Some((first, index)) = self.added.take() {
            if index != first {
                // SAFETY: before reaching this scope's first insertion, the predecessor is another new slot.
                let previous = unsafe { self.core.block.metadata.add(index.get()).as_ref() }
                    .hook
                    .previous
                    .get();
                self.added = Some((first, unsafe { links::Index::new_raw(previous) }));
            }
            slab::Slot {
                core: self.core,
                index,
                phase: phase::Occupied,
            }
            .remove();
        }
    }
}

const _: () = {
    assert!(mem::size_of::<Edit<'static, ()>>() == 2 * mem::size_of::<u32>());
    assert!(mem::size_of::<Option<Edit<'static, ()>>>() == 2 * mem::size_of::<u32>());
    assert!(
        mem::size_of::<Result<Edit<'static, ()>, slab::Full>>()
            == mem::size_of::<Result<Edit<'static, ()>, capacity::Count>>()
    );
    assert!(
        mem::align_of::<Result<Edit<'static, ()>, slab::Full>>()
            == mem::align_of::<Result<Edit<'static, ()>, capacity::Count>>()
    );
    assert!(std::mem::size_of::<Key<'static, ()>>() == std::mem::size_of::<u32>());
    assert!(
        std::mem::size_of::<Scope<'static, 'static, (), ()>>() == 2 * std::mem::size_of::<usize>()
    );
};

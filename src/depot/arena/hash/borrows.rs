use crate::{
    base::num::bounded,
    depot::{
        arena::{
            hash::{self, errors, runtime},
            slab::key,
        },
        queue::links,
    },
};

/// # Safety
/// Retain the original owner and loan for the source's lifetime.
/// Every `core` call must preserve its allocation, slot domain, metadata and admitted live slots.
pub(in crate::depot) unsafe trait Source {
    type Value;
    type Tag;

    fn core(&self) -> &hash::Core<Self::Value, Self::Tag>;
}

// SAFETY: this shared native loan retains the same immutable core and all its live slots.
unsafe impl<T, Tag> Source for &hash::Core<T, Tag> {
    type Value = T;
    type Tag = Tag;

    fn core(&self) -> &hash::Core<T, Tag> {
        self
    }
}

// SAFETY: this original exclusive loan prevents outside mutation for the complete admission.
unsafe impl<T, Tag> Source for &mut hash::Core<T, Tag> {
    type Value = T;
    type Tag = Tag;

    fn core(&self) -> &hash::Core<T, Tag> {
        self
    }
}

pub(in crate::depot) struct Position<S: Source> {
    source: S,
    index: bounded::U32<0, { u32::MAX - 1 }>,
}

pub(in crate::depot) struct Present<S: Source>(Position<S>);

impl<S: Source> Position<S> {
    pub(in crate::depot) fn new(source: S, index: usize) -> Option<Self> {
        if index >= source.core().capacity() {
            return None;
        }
        let index = unsafe { links::Index::new_usize(index) };
        Some(Self {
            source,
            index: bounded::U32::from_one(index.position()),
        })
    }

    fn meta(&self) -> &hash::Meta {
        // SAFETY: this position retains the native loan that admitted its index.
        unsafe { self.source.core().meta(self.index) }
    }

    pub(in crate::depot) fn live(self) -> Option<Present<S>> {
        self.meta().is_live().then_some(Present(self))
    }

    pub(in crate::depot) fn try_live(
        source: S,
        key: key::Handle<runtime::Domain<S::Tag>>,
    ) -> Result<Present<S>, errors::Lookup<S::Tag>> {
        let capacity = source.core().capacity();
        let Some(position) = Self::new(source, key.index() as usize) else {
            return Err(errors::Lookup::Index { key, capacity });
        };
        let meta = position.meta();
        if !meta.is_live() {
            return Err(errors::Lookup::Vacant {
                key,
                actual: meta.generation,
            });
        }
        if meta.generation != key.generation() {
            return Err(errors::Lookup::Generation {
                key,
                actual: meta.generation,
            });
        }
        Ok(Present(position))
    }
}

impl<S: Source> Present<S> {
    pub(in crate::depot) fn key(&self) -> key::Handle<runtime::Domain<S::Tag>> {
        key::Handle::new(self.0.index.get(), self.0.meta().generation)
    }
}

impl<'a, T, Tag> Present<&'a hash::Core<T, Tag>> {
    pub(in crate::depot) fn value(self) -> &'a T {
        // SAFETY: the consumed capability retains the initialized slot's original shared loan.
        unsafe { self.0.source.value(self.0.index) }
    }
}

impl<'a, T, Tag> Present<&'a mut hash::Core<T, Tag>> {
    pub(in crate::depot) fn into_entry(self) -> hash::Entry<'a, T, Tag> {
        // SAFETY: the consumed capability retains the exact exclusive owner and live slot.
        unsafe { hash::Entry::new(self.0.source, self.0.index) }
    }
}

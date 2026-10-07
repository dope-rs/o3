use std::marker;

use crate::{
    buffer,
    buffer::{bytes::scope, pool, pool::owners, resident, storage},
    mem::rc,
};

/// Immutable storage whose selections retain this exact owner's identity.
///
/// Selections cannot escape their owner scope.
/// Source views remain on the owner's thread.
#[repr(transparent)]
pub struct Owner<'id, S> {
    pub(in crate::buffer::bytes::scope) storage: S,
    pub(in crate::buffer::bytes::scope) brand: marker::PhantomData<*mut &'id ()>,
}

impl<'id, S: buffer::raw::Source> Owner<'id, S> {
    pub fn view(&self) -> scope::View<'_, 'id> {
        scope::View {
            bytes: self.storage.as_ref(),
            brand: self.brand,
        }
    }
}

impl<'a, 'id, S: buffer::raw::Source + ?Sized> Owner<'id, &'a S> {
    /// Preserves the original shared borrow beyond this temporary scope owner.
    pub fn borrow(&self) -> scope::View<'a, 'id> {
        scope::View {
            bytes: self.storage.as_ref(),
            brand: self.brand,
        }
    }
}

impl<'id, S: buffer::raw::Store> Owner<'id, &mut S> {
    pub fn borrow(&self) -> scope::View<'_, 'id> {
        scope::View {
            bytes: self.storage.as_ref(),
            brand: self.brand,
        }
    }
}

impl<'id, 'd> Owner<'id, resident::Shared<'d>> {
    /// Retains this same immutable extent and identity through the exact clone.
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            storage: self.storage.try_clone()?,
            brand: self.brand,
        })
    }
}

impl<S> Owner<'_, S> {
    pub fn with<T>(storage: S, run: impl for<'id> FnOnce(Owner<'id, S>) -> T) -> T {
        run(Owner {
            storage,
            brand: marker::PhantomData,
        })
    }

    pub fn into_inner(self) -> S {
        self.storage
    }
}

impl<'id, L> Owner<'id, pool::Span<owners::Plain<L>, pool::Owned, pool::Frozen>> {
    pub fn into_shared(self) -> Owner<'id, storage::Shared> {
        Owner {
            storage: self.storage.into_shared(),
            brand: self.brand,
        }
    }
}

impl<'id, S: buffer::raw::Store> Owner<'id, S> {
    pub fn project(mut self, range: scope::Range<'id>) -> S {
        // SAFETY: this invariant owner identity binds the range to its unchanged storage.
        S::project(unsafe { buffer::raw::Slice::from_parts(&mut self.storage, range) });
        self.storage
    }
}

impl<S: AsRef<[u8]>> AsRef<[u8]> for Owner<'_, S> {
    fn as_ref(&self) -> &[u8] {
        self.storage.as_ref()
    }
}

const _: () = {
    assert!(
        size_of::<Owner<'static, pool::Span<owners::Plain<pool::Layout>, pool::Owned, pool::Frozen>>>(
        ) == size_of::<pool::Span<owners::Plain<pool::Layout>, pool::Owned, pool::Frozen>>()
    );
    assert!(
        size_of::<
            Option<
                Owner<'static, pool::Span<owners::Plain<pool::Layout>, pool::Owned, pool::Frozen>>,
            >,
        >() == size_of::<Option<pool::Span<owners::Plain<pool::Layout>, pool::Owned, pool::Frozen>>>(
        )
    );
    assert!(size_of::<Owner<'static, storage::Shared>>() == size_of::<storage::Shared>());
};

use std::num::NonZero;

use crate::{
    buffer,
    buffer::{
        segments,
        segments::{chain::edit, handles},
    },
    depot::arena::linked::slots,
};

/// One remaining owner retaining its original pooled node.
#[repr(transparent)]
pub struct Single<T: buffer::raw::Segment>(segments::Chain<T>);

impl<T: buffer::raw::Segment> Single<T> {
    /// # Safety
    /// The chain owns exactly one live node in the single state.
    pub(in crate::buffer::segments::chain) const unsafe fn new(chain: segments::Chain<T>) -> Self {
        Self(chain)
    }

    pub fn len(&self) -> NonZero<usize> {
        buffer::raw::Unary::item(self).len()
    }

    pub(in crate::buffer::segments::chain) fn into_item(self) -> segments::Item<T> {
        let mut sequence = self.0;
        // SAFETY: Single owns this sole node until it is consumed.
        let index = unsafe { sequence.key.single_index() };
        edit::Single {
            sequence: &mut sequence,
            index,
        }
        .take()
    }
}

impl<T: buffer::raw::Segment> buffer::raw::Unary<T> for Single<T> {
    fn item(&self) -> &segments::Item<T> {
        buffer::raw::Indexed::get(self)
    }
}

// SAFETY: Single retains its sole initialized node for this complete shared borrow.
unsafe impl<'a, T: buffer::raw::Segment + 'a> buffer::raw::Indexed<'a> for &'a Single<T> {
    type Item = segments::Item<T>;

    fn parts(self) -> (&'a slots::Store<Self::Item>, slots::Index) {
        let sequence = &self.0;
        // SAFETY: Single can only be constructed in the nonempty single state.
        let index = unsafe { sequence.key.single_index() };
        (handles::nodes(&sequence.handle), index)
    }
}

impl<T: buffer::raw::Segment> AsRef<T> for Single<T> {
    fn as_ref(&self) -> &T {
        buffer::raw::Unary::item(self).as_ref()
    }
}

impl<T: buffer::raw::Segment> buffer::Residency for Single<T> {
    fn resident(&self) -> usize {
        buffer::Residency::resident(buffer::raw::Unary::item(self))
    }
}

const _: () = {
    assert!(size_of::<Single<buffer::storage::Shared>>() == 2 * size_of::<usize>());
    assert!(
        size_of::<Option<Single<buffer::storage::Shared>>>()
            == size_of::<Single<buffer::storage::Shared>>()
    );
};

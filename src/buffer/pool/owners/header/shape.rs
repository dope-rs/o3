use std::{alloc::Layout, marker, num};

use crate::{
    alloc,
    buffer::{pool, pool::owners},
};

/// Immutable geometry admitted for one exact backing kind.
#[repr(C)]
pub(in crate::buffer::pool) struct Shape<K> {
    slots: u32,
    capacity: num::NonZero<u32>,
    data_offset: num::NonZero<usize>,
    kind: marker::PhantomData<K>,
}

impl<L: Into<pool::Layout>> Shape<owners::Plain<L>> {
    pub(in crate::buffer::pool::owners) fn new(logical: L) -> Self {
        let public = logical.into();
        let cells = u128::from(public.slots()) * size_of::<owners::Slot>() as u128;
        Self {
            slots: public.slots(),
            capacity: public.capacity(),
            // SAFETY: the admitted layout bounds this positive Header-plus-slots extent.
            data_offset: unsafe {
                num::NonZero::new_unchecked((size_of::<owners::Header>() as u128 + cells) as usize)
            },
            kind: marker::PhantomData,
        }
    }
}

impl Shape<owners::Accounted> {
    pub(in crate::buffer::pool::owners) fn try_new(
        logical: pool::Tally,
    ) -> Result<Self, pool::LayoutError> {
        let (public, tally) = logical.into_parts();
        let slots = public.slots();
        let capacity = public.capacity().get();
        let count = u128::from(slots);
        let bytes = count * u128::from(capacity);
        let cells = count * size_of::<owners::Slot>() as u128;
        let plain = size_of::<owners::Header>() as u128 + cells;
        let rooted = plain + size_of::<owners::Accounts>() as u128;
        let max = alloc::raw::Layout::<owners::Header>::MAX;
        let items = u128::from(tally) * size_of::<owners::Account>() as u128;
        let offset = rooted + items;
        if offset + bytes > max {
            return Err(pool::LayoutError::AccountedData {
                slots,
                capacity,
                tally,
            });
        }
        Ok(Self {
            slots,
            capacity: public.capacity(),
            // SAFETY: the validated extent includes its nonempty Header prefix.
            data_offset: unsafe { num::NonZero::new_unchecked(offset as usize) },
            kind: marker::PhantomData,
        })
    }
}

impl<K> Shape<K> {
    pub(in crate::buffer::pool::owners) const fn allocation(&self) -> Layout {
        let bytes = self.slots as usize * self.capacity.get() as usize;
        let align = num::NonZero::<usize>::MIN.saturating_add(align_of::<owners::Header>() - 1);
        // SAFETY: the closed constructors admit the complete slot product,
        // metadata extent and padded Header alignment together.
        unsafe { alloc::raw::Layout::<u8>::at(self.data_offset.get(), bytes, align).pad_to_align() }
    }

    pub(in crate::buffer::pool::owners) const fn slots(&self) -> u32 {
        self.slots
    }

    pub(in crate::buffer::pool::owners) const fn capacity(&self) -> num::NonZero<u32> {
        self.capacity
    }

    pub(in crate::buffer::pool::owners) const fn data_offset(&self) -> num::NonZero<usize> {
        self.data_offset
    }
}

impl<K> Clone for Shape<K> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<K> Copy for Shape<K> {}

const _: () = {
    let count = u32::MAX as u128;
    let rooted = size_of::<owners::Header>() as u128
        + count * size_of::<owners::Slot>() as u128
        + size_of::<owners::Accounts>() as u128;
    let items = count * size_of::<owners::Account>() as u128;
    assert!(rooted <= alloc::raw::Layout::<owners::Header>::MAX);
    assert!(items <= alloc::raw::Layout::<owners::Account>::MAX);
    assert!(rooted + items <= alloc::raw::Layout::<owners::Header>::MAX);
    assert!(size_of::<Shape<owners::Plain>>() == 16);
    assert!(size_of::<Option<Shape<owners::Plain>>>() == 16);
    assert!(size_of::<Shape<owners::Resident<'static>>>() == 16);
};

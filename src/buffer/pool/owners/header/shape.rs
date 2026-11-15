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
        Self {
            slots: public.slots(),
            capacity: public.capacity(),
            data_offset: pool::Layout::offset(public.slots()),
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
        let cells = count * size_of::<owners::Slot<owners::Resident<'static>>>() as u128;
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

impl Shape<owners::Charged<'_>> {
    pub(in crate::buffer::pool::owners) fn try_new(
        public: pool::Layout,
    ) -> Result<Self, pool::LayoutError> {
        let slots = public.slots();
        let offset = pool::Layout::offset(slots).get() as u128
            + size_of::<crate::buffer::resident::Budget<'_>>() as u128;
        if offset + public.bytes() as u128 > alloc::raw::Layout::<owners::Header>::MAX {
            return Err(pool::LayoutError::AccountedData {
                slots,
                capacity: public.capacity().get(),
                tally: 0,
            });
        }
        Ok(Self {
            slots,
            capacity: public.capacity(),
            // SAFETY: the admitted extent includes its nonempty aligned header.
            data_offset: unsafe { num::NonZero::new_unchecked(offset as usize) },
            kind: marker::PhantomData,
        })
    }
}

impl<K: crate::buffer::raw::Backing> Shape<K> {
    #[inline]
    pub(in crate::buffer::pool::owners) fn from_ptr<S>(core: owners::Ptr<K, S>) -> Self {
        Self {
            slots: core.slots() as u32,
            capacity: core.capacity(),
            data_offset: core.data_offset(),
            kind: marker::PhantomData,
        }
    }
}

impl<K> Shape<K> {
    #[inline]
    pub(in crate::buffer::pool::owners) const fn allocation(&self) -> (usize, Layout) {
        let bytes = self.slots as usize * self.capacity.get() as usize;
        let align = num::NonZero::<usize>::MIN.saturating_add(align_of::<owners::Header>() - 1);
        // SAFETY: the closed constructors admit the complete slot product,
        // metadata extent and padded Header alignment together.
        let layout = unsafe {
            alloc::raw::Layout::<u8>::at(self.data_offset.get(), bytes, align).pad_to_align()
        };
        (bytes, layout)
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
        + count * size_of::<owners::Slot<owners::Resident<'static>>>() as u128
        + size_of::<owners::Accounts>() as u128;
    let items = count * size_of::<owners::Account>() as u128;
    assert!(rooted <= alloc::raw::Layout::<owners::Header>::MAX);
    assert!(items <= alloc::raw::Layout::<owners::Account>::MAX);
    assert!(rooted + items <= alloc::raw::Layout::<owners::Header>::MAX);
    assert!(size_of::<Shape<owners::Plain>>() == 16);
    assert!(size_of::<Option<Shape<owners::Plain>>>() == 16);
    assert!(size_of::<Shape<owners::Resident<'static>>>() == 16);
};

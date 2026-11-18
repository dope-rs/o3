use std::ptr;

use crate::buffer::{pool::owners, resident, storage::regions};

/// The exact backing registration borrowed from a live owner.
#[derive(Clone, Copy)]
pub(in crate::buffer) enum Root<'a, 'd> {
    Allocation(&'a crate::buffer::raw::Header<()>),
    Vec(&'a regions::Header),
    Pool(&'a owners::Slot),
    Resident(&'a crate::buffer::raw::Header<resident::Budget<'d>>),
    Accounted(&'a owners::Account<'d>),
    Provided(&'a resident::Charge<'d>),
    Charged(&'a owners::Slot<owners::Charged<'d>>),
}

impl<'a, 'd> Root<'a, 'd> {
    pub(super) fn same_registration(self, other: Root<'_, '_>) -> bool {
        ptr::eq(self.registration(), other.registration())
    }

    fn registration(self) -> *const () {
        // Each live registration occupies its own nonempty storage. Retyping a
        // resident allocation as plain consumes the sole edge after all loans end.
        match self {
            Self::Allocation(owner) => ptr::from_ref(owner).cast(),
            Self::Vec(owner) => ptr::from_ref(owner).cast(),
            Self::Pool(owner) => ptr::from_ref(owner).cast(),
            Self::Resident(owner) => ptr::from_ref(owner).cast(),
            Self::Accounted(owner) => ptr::from_ref(owner).cast(),
            Self::Provided(owner) => ptr::from_ref(owner).cast(),
            Self::Charged(owner) => ptr::from_ref(owner).cast(),
        }
    }

    #[inline(always)]
    pub(in crate::buffer) fn bytes(self) -> usize {
        match self {
            Self::Allocation(owner) => owner.capacity().get() as usize,
            Self::Vec(owner) => owner.capacity(),
            Self::Pool(owner) => {
                // SAFETY: the plain Tag retains this exact active slot throughout the borrow.
                unsafe { owners::capacity_of::<owners::Plain>(ptr::NonNull::from(owner).cast()) }
                    .get() as usize
            }
            Self::Resident(owner) => owner.capacity().get() as usize,
            Self::Accounted(owner) => owner.bytes(),
            Self::Provided(owner) => owner.capacity(),
            Self::Charged(owner) => {
                // SAFETY: the charged edge retains this exact active slot.
                unsafe {
                    owners::capacity_of::<owners::Charged<'d>>(ptr::NonNull::from(owner).cast())
                }
                .get() as usize
            }
        }
    }
}

const _: () = {
    assert!(size_of::<Root<'static, 'static>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Root<'static, 'static>>>() == 2 * size_of::<usize>());
};

use std::ptr;

use storage::provided;

use crate::buffer::{pool::owners, resident, storage, storage::regions};

/// The exact backing registration borrowed from a live owner.
#[derive(Clone, Copy)]
pub(in crate::buffer) enum Root<'a, 'd> {
    Allocation(&'a crate::buffer::raw::Header<()>),
    Vec(&'a regions::Header),
    Pool(&'a owners::Slot),
    Resident(&'a crate::buffer::raw::Header<resident::Lease<'d>>),
    Accounted(&'a owners::Account<'d>),
    Provided(&'a provided::Charge<'d>),
}

impl<'a, 'd> Root<'a, 'd> {
    pub(in crate::buffer) fn bytes(self) -> usize {
        match self {
            Self::Allocation(owner) => owner.capacity().get() as usize,
            Self::Vec(owner) => owner.capacity(),
            Self::Pool(owner) => owner.capacity().get() as usize,
            Self::Resident(owner) => owner.capacity().get() as usize,
            Self::Accounted(owner) => owner.bytes(),
            Self::Provided(owner) => owner.resident(),
        }
    }
}

impl<'a, 'b, 'd, 'e> PartialEq<Root<'b, 'e>> for Root<'a, 'd> {
    fn eq(&self, other: &Root<'b, 'e>) -> bool {
        match self {
            Self::Allocation(left) => {
                matches!(other, Root::Allocation(right) if ptr::addr_eq(*left, *right))
            }
            Self::Vec(left) => matches!(other, Root::Vec(right) if ptr::addr_eq(*left, *right)),
            Self::Pool(left) => matches!(other, Root::Pool(right) if ptr::addr_eq(*left, *right)),
            Self::Resident(left) => {
                matches!(other, Root::Resident(right) if ptr::addr_eq(*left, *right))
            }
            Self::Accounted(left) => {
                matches!(other, Root::Accounted(right) if ptr::addr_eq(*left, *right))
            }
            Self::Provided(left) => {
                matches!(other, Root::Provided(right) if ptr::addr_eq(*left, *right))
            }
        }
    }
}

impl Eq for Root<'_, '_> {}

const _: () = {
    assert!(size_of::<Root<'static, 'static>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Root<'static, 'static>>>() == 2 * size_of::<usize>());
};

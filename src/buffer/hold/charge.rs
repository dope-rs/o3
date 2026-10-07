use crate::{
    buffer,
    buffer::{hold::Overflow, segments},
};

/// Logical ranges, resident admission bytes, and disposable runtime owners.
/// Split owners may each charge their complete backing: `resident_bytes` is an
/// admission charge, not a unique-allocation count.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Charge {
    pub logical_bytes: usize,
    pub resident_bytes: usize,
    pub owners: usize,
}

impl Charge {
    pub(in crate::buffer) fn from_view<S: buffer::Residency + ?Sized>(
        view: &buffer::View<'_, S>,
    ) -> Self {
        let logical = view.len();
        let resident = if logical == 0 {
            0
        } else {
            view.source.resident()
        };
        Self::from_commit(logical, resident, usize::from(resident > 0))
    }

    /// Constructs an exact delta already committed by an ownership authority.
    /// Partial deltas are valid because the authority has already proved them.
    pub const fn from_commit(logical: usize, resident: usize, owners: usize) -> Self {
        Self {
            logical_bytes: logical,
            resident_bytes: resident,
            owners,
        }
    }

    pub const fn is_empty(self) -> bool {
        self.logical_bytes == 0 && self.resident_bytes == 0 && self.owners == 0
    }

    /// Projects one resident owner. Borrowed and static ranges have no resident
    /// backing and therefore produce no runtime ownership mutation.
    pub const fn from_owner(logical: usize, resident: usize) -> Self {
        if logical == 0 || resident == 0 {
            Self::from_commit(0, 0, 0)
        } else {
            Self::from_commit(logical, resident, 1)
        }
    }

    /// Projects a zero-copy split of an already-accounted logical range.
    pub const fn from_split(resident: usize) -> Self {
        if resident == 0 {
            Self::from_commit(0, 0, 0)
        } else {
            Self::from_commit(0, resident, 1)
        }
    }

    /// Combines two charges without erasing the first overflowing component.
    pub const fn try_add(self, other: Self) -> Result<Self, Overflow> {
        let (logical, overflow) = self.logical_bytes.overflowing_add(other.logical_bytes);
        if overflow {
            return Err(Overflow::Logical {
                left: self.logical_bytes,
                right: other.logical_bytes,
            });
        }
        let (resident, overflow) = self.resident_bytes.overflowing_add(other.resident_bytes);
        if overflow {
            return Err(Overflow::Resident {
                left: self.resident_bytes,
                right: other.resident_bytes,
            });
        }
        let (owners, overflow) = self.owners.overflowing_add(other.owners);
        if overflow {
            return Err(Overflow::Owners {
                left: self.owners,
                right: other.owners,
            });
        }
        Ok(Self::from_commit(logical, resident, owners))
    }
}

/// Borrowed topology and Segment's shared-range contract retain this subset
/// within the chain's admitted length and cardinality.
impl<T: buffer::raw::Segment> From<&segments::Chain<T>> for Charge {
    fn from(chain: &segments::Chain<T>) -> Self {
        let mut logical = 0usize;
        let mut owners = 0usize;
        for owner in chain.segments() {
            if buffer::Residency::resident(owner) != 0 {
                let len = owner.as_ref().len();
                logical += len;
                owners += 1;
            }
        }
        Self::from_commit(logical, buffer::Residency::resident(chain), owners)
    }
}

const _: () = assert!(core::mem::size_of::<Charge>() == 3 * core::mem::size_of::<usize>());

pub(in crate::buffer::resident) mod exact;
pub(in crate::buffer::resident) mod state;

use std::ops;

use storage::{backing, provided, range, regions};

use crate::{
    buffer,
    buffer::{
        bytes::bounded,
        resident,
        resident::copy,
        storage,
        write::{consume, consume::Consume},
    },
    mem::rc,
};

/// Immutable bytes whose accounting or borrow domain remains in `'d`.
#[derive(Debug)]
#[repr(C)]
pub struct Shared<'d> {
    /// Leading owner niche remains available to enclosing payload enums.
    pub(in crate::buffer) region: regions::Shared<resident::State<'d>>,
}
impl<'d> Default for Shared<'d> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'d> Shared<'d> {
    pub const fn new() -> Self {
        Self {
            region: regions::Shared::empty(resident::State::Slice),
        }
    }
    /// Copies an admitted source while retaining its borrow through initialization.
    pub fn try_copy(
        source: &impl bounded::Source,
        budget: &resident::Budget<'d>,
    ) -> Result<Self, copy::Error> {
        Ok(storage::Owned::try_copy(source, budget)?.freeze())
    }

    pub fn as_slice(&self) -> &[u8] {
        self.region.as_slice()
    }

    pub fn get(&self, range: ops::Range<usize>) -> Option<buffer::View<'_, Self>> {
        Some(buffer::View {
            source: self,
            bytes: self.as_slice().get(range)?,
        })
    }
    #[doc(hidden)]
    pub fn backing(&self) -> Option<backing::View<'_, 'd>> {
        backing::View::new(&self.region)
    }
    pub fn clear(&mut self) {
        *self = Self::new();
    }
    pub fn project(
        self,
        visible: ops::Range<usize>,
    ) -> Result<Self, crate::mem::Rejected<Self, range::Error>> {
        buffer::raw::Slice::try_project(self, visible)
    }
    pub fn try_detach(self) -> Result<storage::Shared, Self> {
        self.region.try_detach().map_err(|region| Self { region })
    }
    /// Transfers independent backing or a uniquely owned resident allocation.
    /// Resident transfers refund their domain charge without changing the range.
    /// A backing that cannot become independent is returned unchanged.
    pub fn try_owned(self) -> Result<storage::Shared, Self> {
        self.region.try_own().map_err(|region| Self { region })
    }
    pub fn scope(&self) -> Option<&Self> {
        use resident::State;
        match self.region.owner() {
            State::Owned(_) | State::Borrowed(_) => None,
            State::Slice if self.region.is_empty() => None,
            State::Slice | State::Resident(_) | State::Accounted(_) | State::Provided(_) => {
                Some(self)
            }
        }
    }
    pub fn try_clone(&self) -> Result<Self, rc::Error> {
        Ok(Self {
            region: self.region.try_clone()?,
        })
    }
    pub(in crate::buffer) fn try_share(
        view: regions::NonEmpty<'_, '_, Self>,
    ) -> Result<Self, rc::Error> {
        let view = view.view();
        Ok(Self {
            // SAFETY: the source-bound selection retains the same immutable
            // region throughout retention; no independent owner can be supplied.
            region: unsafe { view.source.region.try_view(view.as_slice())? },
        })
    }
}
// SAFETY: shared access preserves the original immutable extent.
unsafe impl buffer::raw::Source for Shared<'_> {}
impl buffer::Residency for Shared<'_> {
    #[inline]
    fn resident(&self) -> usize {
        self.region.resident()
    }
}
impl Consume for Shared<'_> {
    fn remaining(&self) -> usize {
        self.region.len()
    }
    fn try_consume(&mut self, amount: usize) -> Result<(), consume::Error> {
        buffer::raw::Selection::try_new(self, amount).map(buffer::raw::Selection::consume)
    }
    fn consume_max(&mut self, max: usize) -> usize {
        buffer::raw::Store::advance_max(self, max)
    }
}
impl AsRef<[u8]> for Shared<'_> {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}
impl ops::Deref for Shared<'_> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        &self.region
    }
}
impl<'d> From<bounded::Value<storage::Shared<Option<provided::Account<'d>>>>> for Shared<'d> {
    fn from(bytes: bounded::Value<storage::Shared<Option<provided::Account<'d>>>>) -> Self {
        bytes.0.into()
    }
}
const _: () = {
    assert!(size_of::<Shared<'static>>() == 4 * size_of::<usize>());
    assert!(size_of::<Option<Shared<'static>>>() == 4 * size_of::<usize>());
};

const _: () = {
    assert!(
        size_of::<crate::buffer::raw::Consumed<'static, 'static, Shared<'static>, 8>>()
            == 2 * size_of::<usize>()
    );
    assert!(
        size_of::<Option<crate::buffer::raw::Consumed<'static, 'static, Shared<'static>, 8>>>()
            == 2 * size_of::<usize>()
    );
};

// SAFETY: every sealed resident constructor retains its exact immutable range and domain.
unsafe impl buffer::raw::Store for Shared<'_> {
    fn empty() -> Option<Self> {
        Some(Self::new())
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        let (target, range) = slice.into_parts();
        // SAFETY: the exclusive operation retains this owner's selected initialized extent.
        unsafe { target.region.restrict(range.into_inner()) };
    }
}

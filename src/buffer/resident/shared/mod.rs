use regions::Initialize as _;

use crate::buffer::{pool::owners, storage::regions};
pub(in crate::buffer::resident) mod exact;
pub(in crate::buffer::resident) mod state;

use std::ops;

use storage::{backing, provided, range};

use crate::{
    buffer,
    buffer::{
        bytes::bounded,
        pool, resident,
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
    pub(in crate::buffer) region: regions::Value<resident::State<'d>>,
}
impl<'d> Default for Shared<'d> {
    fn default() -> Self {
        Self::new()
    }
}

impl<'d> Shared<'d> {
    pub const fn new() -> Self {
        Self {
            region: regions::Value::empty(resident::State::Slice),
        }
    }
    /// Copies an admitted source while retaining its borrow through initialization.
    pub fn try_copy(
        source: &impl bounded::Source,
        budget: &resident::Budget<'d>,
    ) -> Result<Self, copy::Error> {
        let Some(source) = buffer::raw::Slices::from_source(source) else {
            return Ok(Self::new());
        };
        Ok(source
            .try_init::<resident::Lease<'d>>(*budget)?
            .into_buf()
            .into())
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
        match self.region.try_detach() {
            Ok(region) => Ok(storage::Shared { region }),
            Err(region) => Err(Self { region }),
        }
    }
    /// Transfers independent backing or a uniquely owned resident allocation.
    /// Resident transfers refund their domain charge without changing the range.
    /// A backing that cannot become independent is returned unchanged.
    pub fn try_owned(self) -> Result<storage::Shared, Self> {
        match self.region.try_own() {
            Ok(region) => Ok(storage::Shared { region }),
            Err(region) => Err(Self { region }),
        }
    }
    pub fn scope(&self) -> resident::Scope<'_, 'd> {
        use resident::{Scope, State};
        match self.region.owner() {
            State::Owned(_) | State::Borrowed(_) => Scope::Independent,
            State::Slice if self.region.is_empty() => Scope::Independent,
            State::Slice | State::Resident(_) | State::Accounted(_) | State::Provided(_) => {
                Scope::Bound(self)
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
            region: unsafe { view.source().region.try_view(view.as_slice())? },
        })
    }
}
// SAFETY: shared access preserves the original immutable extent.
unsafe impl buffer::raw::Source for Shared<'_> {}
impl buffer::Residency for Shared<'_> {
    fn resident(&self) -> usize {
        self.region.owner().resident()
    }
}
impl<'a: 'd, 'd> From<&'a [u8]> for Shared<'d> {
    fn from(bytes: &'a [u8]) -> Self {
        Self {
            region: bytes.into(),
        }
    }
}
impl<'d> From<storage::Shared> for Shared<'d> {
    fn from(shared: storage::Shared) -> Self {
        Self {
            region: shared.region.into(),
        }
    }
}
impl<'d> From<storage::Shared<crate::buffer::raw::Rc<resident::Lease<'d>>>> for Shared<'d> {
    fn from(shared: storage::Shared<crate::buffer::raw::Rc<resident::Lease<'d>>>) -> Self {
        Self {
            region: shared.region.into(),
        }
    }
}
impl<'d, L> From<storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'d>>>>
    for Shared<'d>
{
    fn from(shared: storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'d>>>) -> Self {
        Self {
            region: shared.region.into(),
        }
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
    type Target = regions::Bytes;
    fn deref(&self) -> &Self::Target {
        &self.region
    }
}
impl<'d> From<storage::Shared<Option<provided::Account<'d>>>> for Shared<'d> {
    fn from(shared: storage::Shared<Option<provided::Account<'d>>>) -> Self {
        Self {
            region: shared.region.into(),
        }
    }
}
impl<'d> From<bounded::Value<storage::Shared<Option<provided::Account<'d>>>>> for Shared<'d> {
    fn from(bytes: bounded::Value<storage::Shared<Option<provided::Account<'d>>>>) -> Self {
        Self {
            region: bytes.0.region.into(),
        }
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

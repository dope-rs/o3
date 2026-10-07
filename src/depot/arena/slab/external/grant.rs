use crate::depot::arena::slab::{self, Vacancy, entries, external};

/// Owns a physical reservation while retaining the unspent external identity.
#[must_use = "a slab vacancy grant must be reserved or released"]
pub struct Grant<
    'a,
    T,
    G: external::Generation,
    Tag = (),
    const PHYSICAL_MAX: u32 = { u32::MAX },
    const PARTITIONS: usize = 1,
> {
    inner: slab::Vacant<Pending<'a, T, G, PHYSICAL_MAX, PARTITIONS>, Tag>,
    state: &'a mut Option<G>,
    generation: G,
}

impl<'a, T, G: external::Generation, Tag, const PHYSICAL_MAX: u32, const PARTITIONS: usize>
    Grant<'a, T, G, Tag, PHYSICAL_MAX, PARTITIONS>
{
    pub(super) fn try_new(
        state: &'a mut Option<G>,
        acquire: impl FnOnce() -> Option<slab::Vacant<Pending<'a, T, G, PHYSICAL_MAX, PARTITIONS>, Tag>>,
    ) -> Result<Self, Vacancy> {
        let epoch = state.ok_or(Vacancy::IdentityExhausted)?;
        let inner = acquire().ok_or(Vacancy::Full)?;
        Ok(Self {
            inner,
            state,
            generation: epoch,
        })
    }

    pub fn reserve(
        self,
    ) -> external::Vacant<Pending<'a, T, G, PHYSICAL_MAX, PARTITIONS>, Tag, true, G> {
        *self.state = self.generation.next();
        self.inner.with_state(self.generation)
    }
}

type Pending<'a, T, G, const MAX: u32, const N: usize> =
    entries::Pending<'a, external::Stamped<T, G>, MAX, entries::Exclusive<true>, N>;

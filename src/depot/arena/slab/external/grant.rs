use crate::depot::arena::slab::{self, Vacancy, external};

/// Owns a physical reservation while retaining the unspent external identity.
pub type Grant<'a, T, G, Tag = (), const PARTITIONS: usize = 1> =
    Reservation<'a, slab::Vacant<external::Pending<'a, T, G, PARTITIONS>, Tag>, G>;

#[must_use = "a slab vacancy grant must be reserved or released"]
pub struct Reservation<'a, B, G: external::Generation> {
    pub(in crate::depot::arena::slab::external) inner: B,
    state: &'a mut Option<G>,
    generation: G,
}

impl<'a, B, G: external::Generation> Reservation<'a, B, G> {
    pub(super) fn try_new(
        state: &'a mut Option<G>,
        acquire: impl FnOnce() -> Option<B>,
    ) -> Result<Self, Vacancy> {
        let epoch = state.ok_or(Vacancy::IdentityExhausted)?;
        let inner = acquire().ok_or(Vacancy::Full)?;
        Ok(Self {
            inner,
            state,
            generation: epoch,
        })
    }

    pub(super) fn issue(self) -> (B, G) {
        *self.state = self.generation.next();
        (self.inner, self.generation)
    }
}

impl<'a, T, G: external::Generation, Tag, const PARTITIONS: usize>
    Grant<'a, T, G, Tag, PARTITIONS>
{
    pub fn reserve(self) -> external::VacantSlot<'a, T, G, Tag, PARTITIONS> {
        let (inner, generation) = self.issue();
        inner.with_state(generation)
    }
}

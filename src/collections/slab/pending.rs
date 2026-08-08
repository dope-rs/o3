use crate::collections::slab::{self, core};

pub(super) struct Pending<'a, T, G: slab::GenerationState, M: core::Mode> {
    core: &'a core::Core<T, G, M>,
    ticket: Option<core::Ticket<G>>,
}

impl<'a, T, G: slab::GenerationState, M: core::Mode> Pending<'a, T, G, M> {
    pub(super) fn new(core: &'a core::Core<T, G, M>, ticket: core::Ticket<G>) -> Self {
        Self {
            core,
            ticket: Some(ticket),
        }
    }

    pub(super) fn commit(mut self, value: T) {
        self.core
            .reservations()
            .commit(unsafe { self.ticket.take().unwrap_unchecked() }, value);
    }
}

impl<T, G: slab::GenerationState, M: core::Mode> Drop for Pending<'_, T, G, M> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.core.reservations().rollback(ticket);
        }
    }
}

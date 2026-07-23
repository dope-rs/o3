use super::GenerationState;
use super::core::{Mode, SlabCore, Ticket};

pub(super) struct Pending<'a, T, G: GenerationState, M: Mode> {
    core: &'a SlabCore<T, G, M>,
    ticket: Option<Ticket<G>>,
}

impl<'a, T, G: GenerationState, M: Mode> Pending<'a, T, G, M> {
    pub(super) fn new(core: &'a SlabCore<T, G, M>, ticket: Ticket<G>) -> Self {
        Self {
            core,
            ticket: Some(ticket),
        }
    }

    pub(super) fn commit(mut self, value: T) {
        self.core
            .commit(unsafe { self.ticket.take().unwrap_unchecked() }, value);
    }
}

impl<T, G: GenerationState, M: Mode> Drop for Pending<'_, T, G, M> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.core.rollback(ticket);
        }
    }
}

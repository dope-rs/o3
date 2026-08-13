use crate::collections::slab::sealed::{self, core};

pub(super) struct Pending<'a, T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>
{
    core: &'a core::Core<T, G, M, PARTITIONS>,
    ticket: Option<core::Ticket<G>>,
}

impl<'a, T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize>
    Pending<'a, T, G, M, PARTITIONS>
{
    pub(super) fn new(core: &'a core::Core<T, G, M, PARTITIONS>, ticket: core::Ticket<G>) -> Self {
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

impl<T, G: sealed::GenerationState, M: core::Mode, const PARTITIONS: usize> Drop
    for Pending<'_, T, G, M, PARTITIONS>
{
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.core.reservations().rollback(ticket);
        }
    }
}

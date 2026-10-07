//! Consuming joins between caller-owned resident FIFOs and their active lane ring.

use crate::{
    depot,
    depot::queue::{links, round::phase},
};

/// Borrowed projections of one resident lane. Mutation requires a complete source proof.
#[derive(Debug)]
pub struct Join<'a, H: ?Sized, L: ?Sized> {
    pub hooks: &'a H,
    pub lanes: &'a L,
    pub entries: &'a links::Chain,
    pub active: &'a links::Cycle,
    pub index: links::Index,
}

impl<'a, H: depot::raw::Hooks + ?Sized, L: depot::raw::Hooks + ?Sized> Join<'a, H, L> {
    pub fn insert(source: depot::raw::Proof<(Self, links::Index), phase::Vacant>) {
        let (lane, index) = source.into_inner();
        // SAFETY: the source retains the exact exclusively vacant resident hook.
        let vacant = unsafe { links::Detached::new(lane.hooks.hook_raw(index), index) };
        // SAFETY: the same source retains its detached resident and reciprocal destination.
        Self::push(unsafe { depot::raw::Proof::new((lane, vacant)) });
    }

    pub fn push(source: depot::raw::Proof<(Self, links::Detached<'a>), phase::Vacant>) {
        let (lane, vacant) = source.into_inner();
        let activate = lane.entries.is_empty();
        // SAFETY: the source retains this exact vacant hook and reciprocal destination.
        unsafe { vacant.push(lane.entries, lane.hooks) };
        if activate {
            // SAFETY: the empty lane was absent from its active ring.
            unsafe { lane.active.push(lane.lanes, lane.index) };
        }
    }

    pub fn remove(
        source: depot::raw::Proof<(Self, links::Index), phase::Linked>,
    ) -> links::Detached<'a> {
        let (lane, index) = source.into_inner();
        // SAFETY: the source retains this resident's exact reciprocal membership.
        let vacant = unsafe { lane.entries.unlink(lane.hooks, index) };
        if lane.entries.is_empty() {
            // SAFETY: the last resident's removal retains this lane's active membership.
            unsafe { lane.active.unlink_key(lane.lanes, lane.index) };
        }
        vacant
    }

    pub fn pop(source: depot::raw::Proof<Self, phase::Ready>) -> Option<links::Detached<'a>> {
        let lane = source.into_inner();
        let index = lane.entries.first()?;
        // SAFETY: the admitted reciprocal FIFO retains its selected front until commit.
        Some(unsafe { lane.remove_front::<false, false>(index) })
    }

    pub fn active<const ROTATE: bool>(
        source: depot::raw::Proof<Self, phase::Active>,
    ) -> links::Detached<'a> {
        let source = Self::front(source);
        let (lane, index) = source.into_inner();
        // SAFETY: the source retains the FIFO front and the active ring's exact front.
        unsafe { lane.remove_front::<true, ROTATE>(index) }
    }

    /// Retains an active lane together with its nonempty FIFO's current front.
    pub fn front(
        source: depot::raw::Proof<Self, phase::Active>,
    ) -> depot::raw::Proof<(Self, links::Index), phase::Head> {
        let lane = source.into_inner();
        // SAFETY: active-front membership proves this exact lane remains nonempty.
        let index = unsafe { lane.entries.front_key() };
        // SAFETY: the source's stable active membership retains this exact FIFO front.
        unsafe { depot::raw::Proof::new((lane, index)) }
    }

    pub fn head<const ROTATE: bool>(
        source: depot::raw::Proof<(Self, links::Index), phase::Head>,
    ) -> links::Detached<'a> {
        let (lane, index) = source.into_inner();
        // SAFETY: Head retains the exact FIFO front through this consuming commit.
        unsafe { lane.remove_front::<false, ROTATE>(index) }
    }

    /// # Safety
    /// The resident is this FIFO's front. ACTIVE proves the active-ring front.
    /// ROTATE advances the selected lane only if removal leaves the FIFO nonempty.
    unsafe fn remove_front<const ACTIVE: bool, const ROTATE: bool>(
        self,
        index: links::Index,
    ) -> links::Detached<'a> {
        let change = unsafe { depot::raw::Pop::at(self.entries, self.hooks, index) };
        let (_, next) = change.edge().links();
        let vacant = depot::raw::Removal::commit(change);
        if next == links::Index::END {
            if ACTIVE {
                unsafe { self.active.pop_front(self.lanes, self.index) };
            } else {
                unsafe { self.active.unlink_key(self.lanes, self.index) };
            }
        } else if ROTATE {
            let first = if ACTIVE {
                self.index
            } else {
                // SAFETY: the successor keeps this lane in its nonempty active ring.
                unsafe { self.active.first_live() }
            };
            // SAFETY: removal leaves this lane in the reciprocal ring, and first
            // retains its exact front; ACTIVE additionally proves this lane is first.
            unsafe {
                self.active
                    .move_back::<_, ACTIVE>(self.lanes, self.index, first)
            };
        }
        vacant
    }
}

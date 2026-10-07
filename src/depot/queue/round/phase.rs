use crate::depot;

/// Sole authority retains reciprocal resident/lane stores and the indexed FIFO.
/// Active membership exactly reflects nonemptiness; hooks and heads are disjoint.
/// Projections and destruction cannot mutate or reenter this topology.
#[derive(Debug)]
pub enum Ready {}

/// Ready holds and the supplied index or detached capability is exclusively vacant
/// in this exact resident store until the consuming insertion.
#[derive(Debug)]
pub enum Vacant {}

/// Ready holds and the supplied resident index is a member of this exact FIFO
/// until the consuming removal; returned capabilities retain its original lifetime.
#[derive(Debug)]
pub enum Linked {}

/// Ready holds; the supplied resident is this FIFO's current front. These
/// memberships remain unchanged until the consuming transition.
#[derive(Debug)]
pub enum Head {}

/// Ready holds and this lane is the active ring's front, proving its FIFO nonempty.
/// Both memberships remain unchanged until the consuming removal.
#[derive(Debug)]
pub enum Active {}

impl depot::raw::Sealed for Ready {}
impl depot::raw::Sealed for Vacant {}
impl depot::raw::Sealed for Linked {}
impl depot::raw::Sealed for Head {}
impl depot::raw::Sealed for Active {}
impl depot::Predicate for Ready {}
impl depot::Predicate for Vacant {}
impl depot::Predicate for Linked {}
impl depot::Predicate for Head {}
impl depot::Predicate for Active {}

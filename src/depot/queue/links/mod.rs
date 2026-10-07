mod detached;
mod edge;
mod list;

pub use detached::Detached;
pub use edge::Edge;
pub use list::List;

mod index;
pub use index::Index;

mod hook;
pub use hook::Hook;

mod free;
pub use free::Free;

mod cycle;
pub use cycle::Cycle;

mod chain;
pub use chain::Chain;

/// The retained hook and index are exclusively detached from every topology with no other capability.
/// This authority holds until consumption; its borrowed outputs retain the same hook lifetime.
#[derive(Debug)]
pub enum Vacant {}

impl crate::depot::Predicate for Vacant {}

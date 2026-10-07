use crate::depot::queue::indexed::fifo;

/// Exclusive phase proof for one indexed FIFO position.
pub enum Entry<'a, T, I: crate::depot::raw::Compact = u32> {
    Vacant(fifo::Slot<'a, T, fifo::Vacant, I>),
    Occupied(fifo::Slot<'a, T, fifo::Occupied, I>),
}

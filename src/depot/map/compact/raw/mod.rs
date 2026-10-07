use crate::{
    alloc::{self, tail},
    depot::map::compact::{self, Found, Layout, Removed, Value},
};

mod simd;
mod store;
pub(super) use store::Store;

/// # Safety
/// Layout and initialized storage, including mirrored controls, must stay stable.
/// Found buckets hold unique live bounded payloads in Robin Hood order, with at least one spare bucket.
pub unsafe trait Read: Sized {
    fn layout(&self) -> Layout;
    fn bytes(&self) -> &[u8];
}

/// Resolves records in this source to their canonical, stable hashes.
///
/// # Safety
/// Every stored or admitted payload must retain the hash that places it in this directory.
pub unsafe trait Source: Read {
    fn hash(value: Value<'_, Self>) -> u64;
}

/// # Safety
/// Returned items must retain every source borrow they expose and must not permit directory invalidation.
pub unsafe trait Query: Source {
    type Item;
    fn get(value: Value<'_, Self>) -> Self::Item;
}

/// Mutably borrows only this owner's directory; the associated records stay stable.
///
/// # Safety
/// The mutable bytes must be the same exclusively owned plane returned by `bytes`.
pub unsafe trait Write: Read {
    /// # Safety
    /// Mutation preserves this exact member, hash, and directory bucket positions.
    /// Implementations must return this entry's exact retained owner.
    unsafe fn owner_mut(entry: &mut Found<Self>) -> &mut Self {
        &mut entry.owner
    }
    /// # Safety
    /// Mutations must preserve or consume this source's directory invariants.
    unsafe fn bytes_mut(&mut self) -> &mut [u8];

    /// Resets the control mirror retained by this exact directory owner.
    /// # Safety
    /// Retire this directory's indexed memberships before exposing its owner again.
    unsafe fn clear(&mut self) {
        let layout = self.layout();
        // SAFETY: Write retains this layout and its exclusively owned initialized extent.
        unsafe { init(layout, self.bytes_mut()) };
    }
}

/// Exclusively owns one live record and its membership in this directory.
///
/// # Safety
/// `input` must retain the same indexed member for this operation's entire lifetime.
pub unsafe trait Member: Remove {
    fn input(&self) -> Value<'_, Self>;
}

/// Consumes removal evidence into this source owner's next state.
///
/// # Safety
/// Completion must retire the removed membership before exposing the owner again.
pub unsafe trait Remove: Write + Source {
    /// # Safety
    /// Consume removal into its corresponding record-state transition before exposing the owner.
    /// Implementations must return this removed entry's exact retained owner.
    unsafe fn into_owner(removed: Removed<Self>) -> Self {
        removed.owner
    }
    type Output;
    fn removed(removed: Removed<Self>) -> Self::Output;
}

/// # Safety
/// Owns one stable, bounded, initialized record absent from this nonempty directory.
/// Insertion must preserve an empty bucket; completion publishes the record exactly once.
pub unsafe trait Insert: Write + Source {
    type Output;
    fn input(&self) -> Value<'_, Self>;
    /// # Safety
    /// The directory must have just indexed this operation's record exactly once.
    unsafe fn inserted(self) -> Self::Output;
}

/// One admitted header-and-byte-tail allocation ending at this directory.
/// The prefix and payload bytes start zeroed; controls and mirrors start empty.
pub struct Plan<H> {
    layout: compact::Layout,
    memory: tail::Plan<H, u8>,
}

impl<H> Plan<H> {
    pub fn new(layout: compact::Layout) -> Result<Self, alloc::Overflow> {
        Ok(Self {
            layout,
            memory: tail::Plan::try_new(layout.end())?,
        })
    }

    pub fn try_allocate(self, header: H) -> Result<tail::Block<H, u8>, alloc::Exhausted> {
        let mut block = self.memory.try_allocate()?.repeat(header, 0);
        // SAFETY: admission binds this layout's complete extent to this exact
        // allocation. Its initialized tail is exclusive and not yet published.
        unsafe { init(self.layout, block.as_mut_slice()) };
        Ok(block)
    }
}

impl<H> Clone for Plan<H> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H> Copy for Plan<H> {}

// SAFETY: callers retain the exact admitted extent and exclusive initialized bytes.
unsafe fn init(layout: Layout, bytes: &mut [u8]) {
    if layout.buckets != 0 {
        unsafe {
            bytes
                .as_mut_ptr()
                .add(layout.start as usize)
                .write_bytes(compact::EMPTY, layout.control_len());
        }
    }
}

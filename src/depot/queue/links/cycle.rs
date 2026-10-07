use std::cell;

use crate::depot::queue::links;
/// One-word head of a caller-owned circular indexed order.
#[derive(Debug)]
#[repr(transparent)]
pub struct Cycle(cell::Cell<u32>);

impl Default for Cycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Cycle {
    pub const fn new() -> Self {
        Self(cell::Cell::new(links::Index::END))
    }

    pub fn front(&self) -> Option<usize> {
        self.first().map(links::Index::get)
    }

    pub fn first(&self) -> Option<links::Index> {
        links::Index::decode(self.0.get())
    }

    /// # Safety
    /// This exact reciprocal ring remains nonempty for the immediate operation.
    pub(in crate::depot) unsafe fn first_live(&self) -> links::Index {
        unsafe { links::Index::new_raw(self.0.get()) }
    }

    pub fn is_empty(&self) -> bool {
        self.0.get() == links::Index::END
    }

    /// Advances a proven non-empty reciprocal ring without a recoverable path.
    /// # Safety
    /// `index` is this ring's live head and its successor belongs to this exact store.
    pub(in crate::depot) unsafe fn next_live<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) -> links::Index {
        let hook = unsafe { hooks.hook_raw(index) };
        let next = hook.next.get();
        let next = if next == links::Index::END {
            index.raw()
        } else {
            next
        };
        self.0.set(next);
        // SAFETY: a singleton retains its live head; otherwise reciprocity proves its successor.
        unsafe { links::Index::new_raw(next) }
    }

    /// # Safety
    /// `index` belongs to this reciprocal ring and store, and `first` is its
    /// current front. FRONT additionally proves that index is that front.
    pub(in crate::depot) unsafe fn move_back<
        H: crate::depot::raw::Hooks + ?Sized,
        const FRONT: bool,
    >(
        &self,
        hooks: &H,
        index: links::Index,
        first: links::Index,
    ) {
        if FRONT || first == index {
            unsafe { self.next_live(hooks, first) };
        } else {
            let first = unsafe { hooks.hook_raw(first) };
            if first.prev_index() != Some(index) {
                unsafe { self.unlink_key(hooks, index) };
                unsafe { self.push(hooks, index) };
            }
        }
    }

    /// Appends one proven-vacant hook without erasing its compact key; a singleton uses END endpoints to distinguish parked state.
    /// # Safety
    /// The ring is reciprocal and `index` is vacant in this exact hook store.
    pub(in crate::depot) unsafe fn push<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) {
        let raw = index.raw();
        // SAFETY: caller proves index is a stable vacant hook in this store.
        let hook = unsafe { hooks.hook_raw(index) };
        let head = self.0.get();
        if head == links::Index::END {
            hook.previous.set(links::Index::END);
            hook.next.set(links::Index::END);
            self.0.set(raw);
            return;
        }
        // SAFETY: a reciprocal nonempty ring's head is a live hook in this store.
        let first = unsafe { hooks.hook_raw(links::Index::new_raw(head)) };
        let tail = match first.previous.get() {
            links::Index::END => head,
            tail => tail,
        };
        // SAFETY: reciprocity guarantees the head's previous edge is the live tail.
        let last = unsafe { hooks.hook_raw(links::Index::new_raw(tail)) };
        hook.previous.set(tail);
        hook.next.set(head);
        last.next.set(raw);
        first.previous.set(raw);
    }

    /// Detaches one proven member without erasing its compact key.
    /// # Safety
    /// The ring is reciprocal and `index` belongs to this exact ring/hook store.
    pub(in crate::depot) unsafe fn unlink_key<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) {
        let advance = self.0.get() == index.raw();
        // SAFETY: caller proves index is a live member in this exact ring/store;
        // `advance` records whether that member is the current head.
        unsafe { self.remove(hooks, index, advance) };
    }

    /// Detaches a proven front without erasing its compact key.
    /// # Safety
    /// `index` is the live reciprocal front of this exact ring/hook store.
    pub(in crate::depot) unsafe fn pop_front<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
    ) {
        // SAFETY: caller proves index is the reciprocal front.
        unsafe { self.remove(hooks, index, true) };
    }

    /// # Safety
    /// `index` belongs to this reciprocal ring and store. If it has neighbors,
    /// `advance` states whether it is the head.
    unsafe fn remove<H: crate::depot::raw::Hooks + ?Sized>(
        &self,
        hooks: &H,
        index: links::Index,
        advance: bool,
    ) {
        let raw = index.raw();
        // SAFETY: caller proves index is a live member in this exact store.
        let hook = unsafe { hooks.hook_raw(index) };
        let next = hook.next.get();
        if next == links::Index::END {
            self.0.set(links::Index::END);
        } else {
            let previous = hook.previous.get();
            // SAFETY: reciprocal endpoint membership guarantees live neighbors.
            let prev = unsafe { hooks.hook_raw(links::Index::new_raw(previous)) };
            // SAFETY: reciprocal endpoint membership guarantees live neighbors.
            let next_ref = unsafe { hooks.hook_raw(links::Index::new_raw(next)) };
            let single = previous == next;
            prev.next.set(if single { links::Index::END } else { next });
            next_ref
                .previous
                .set(if single { links::Index::END } else { previous });
            if advance {
                self.0.set(next);
            }
        }
        hook.previous.set(raw);
        hook.next.set(links::Index::END);
    }
}

const _: () = assert!(std::mem::size_of::<Cycle>() == std::mem::size_of::<u32>());

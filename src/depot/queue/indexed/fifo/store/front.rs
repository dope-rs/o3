use crate::depot::queue::{indexed::fifo, links};
/// The live head retained under its exact FIFO's unchanged borrow.
#[must_use = "a front capability retains its exact FIFO head"]
pub(in crate::depot::queue::indexed) struct Front<'a, T> {
    entry: fifo::NodeEntry<'a, T, links::Chain<u32>>,
}

impl<'a, T> Front<'a, T> {
    /// # Safety
    /// `entry` remains this store's live head for the capability's lifetime.
    pub(super) const unsafe fn new(entry: fifo::NodeEntry<'a, T, links::Chain<u32>>) -> Self {
        Self { entry }
    }

    pub(in crate::depot::queue::indexed) fn get(&self) -> &T {
        let node = self.entry.slot();
        unsafe { (*node.value.get()).assume_init_ref() }
    }

    pub(in crate::depot::queue::indexed) fn take(
        self,
    ) -> (fifo::NodeEntry<'a, T, links::Chain<u32>>, T) {
        let store = self.entry.store();
        // SAFETY: this capability retains the store's unchanged live head.
        crate::depot::raw::Removal::commit(unsafe {
            crate::depot::raw::Pop::at(store.state(), store, self.entry.index())
        })
        .idle();
        let value = unsafe { (*self.entry.slot().value.get()).assume_init_read() };
        (self.entry, value)
    }
}

const _: () = {
    assert!(size_of::<Front<'static, u64>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Front<'static, u64>>>() == 2 * size_of::<usize>());
};

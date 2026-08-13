use std::cell;

/// A single-threaded bounded ledger for explicitly acquired and released units.
pub struct Ledger {
    limit: usize,
    used: cell::Cell<usize>,
    _thread: crate::ThreadBound,
}

impl Ledger {
    pub fn new(limit: usize) -> Self {
        use crate::ThreadBound;
        Self {
            limit,
            used: cell::Cell::new(0),
            _thread: ThreadBound::NEW,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn used(&self) -> usize {
        self.used.get()
    }

    pub fn available(&self) -> usize {
        self.limit - self.used.get()
    }

    pub fn try_acquire(&self, amount: usize) -> bool {
        let Some(used) = self.used.get().checked_add(amount) else {
            return false;
        };
        if used > self.limit {
            return false;
        }
        self.used.set(used);
        true
    }

    pub fn release(&self, amount: usize) {
        let used = self.used.get();
        assert!(used >= amount, "cannot release credits that are not held");
        self.used.set(used - amount);
    }
}

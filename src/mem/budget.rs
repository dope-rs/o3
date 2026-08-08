use std::cell;

pub struct Bytes {
    limit: usize,
    used: cell::Cell<usize>,
    _thread: crate::ThreadBound,
}

impl Bytes {
    pub fn new(limit: usize) -> Self {
        use crate::ThreadBound;
        Self {
            limit,
            used: cell::Cell::new(0),
            _thread: ThreadBound::NEW,
        }
    }

    pub fn handle(&self) -> Handle<'_> {
        Handle(self)
    }
}

#[derive(Clone, Copy)]
pub struct Handle<'d>(&'d Bytes);

impl<'d> Handle<'d> {
    fn budget(self) -> &'d Bytes {
        self.0
    }

    pub fn try_acquire(self, amount: usize) -> Option<Lease<'d>> {
        let budget = self.budget();
        let used = budget.used.get().checked_add(amount)?;
        if used > budget.limit {
            return None;
        }
        budget.used.set(used);
        Some(Lease {
            budget: self,
            amount,
        })
    }

    fn release(self, amount: usize) {
        let budget = self.budget();
        assert!(budget.used.get() >= amount, "byte budget underflow");
        budget.used.set(budget.used.get() - amount);
    }
}

pub struct Lease<'d> {
    budget: Handle<'d>,
    amount: usize,
}

impl Drop for Lease<'_> {
    fn drop(&mut self) {
        self.budget.release(self.amount);
    }
}

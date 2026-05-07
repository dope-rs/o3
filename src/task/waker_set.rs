use std::task::Waker;

pub struct WakerSet {
    wakers: Vec<Waker>,
}

impl WakerSet {
    #[must_use]
    pub const fn new() -> Self {
        Self { wakers: Vec::new() }
    }

    #[must_use]
    pub fn with_capacity(n: usize) -> Self {
        Self {
            wakers: Vec::with_capacity(n),
        }
    }

    pub fn register(&mut self, waker: &Waker) {
        if !self.wakers.iter().any(|w| w.will_wake(waker)) {
            self.wakers.push(waker.clone());
        }
    }

    #[must_use]
    pub fn take(&mut self) -> Self {
        Self {
            wakers: std::mem::take(&mut self.wakers),
        }
    }

    pub fn wake_all(self) {
        for w in self.wakers {
            w.wake();
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.wakers.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.wakers.len()
    }
}

impl Default for WakerSet {
    fn default() -> Self {
        Self::new()
    }
}

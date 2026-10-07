use std::cmp::Ordering;

pub struct Priority<'a> {
    pub value: u32,
    compare: &'a dyn Fn(),
    release: &'a dyn Fn(),
}

impl<'a> Priority<'a> {
    pub fn new(value: u32, compare: &'a dyn Fn(), release: &'a dyn Fn()) -> Self {
        Self {
            value,
            compare,
            release,
        }
    }
}

impl PartialEq for Priority<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Priority<'_> {}

impl PartialOrd for Priority<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Priority<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.compare)();
        self.value.cmp(&other.value)
    }
}

impl Drop for Priority<'_> {
    fn drop(&mut self) {
        (self.release)();
    }
}

use crate::collections::queue::slot;

pub struct Vacant<'queue, T>(slot::Vacancy<'queue, T>);

impl<'queue, T> Vacant<'queue, T> {
    pub(super) fn new(entry: slot::Vacancy<'queue, T>) -> Self {
        Self(entry)
    }

    pub fn push_front(self, value: T) {
        self.0.push_front(value);
    }

    pub fn push_back(self, value: T) {
        self.0.push_back(value);
    }
}

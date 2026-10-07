/// An unchanged input retained with its exact rejection cause.
#[must_use = "recover the input or explicitly discard it"]
#[derive(Debug)]
pub struct Rejected<T, E> {
    value: T,
    error: E,
}

impl<T, E> Rejected<T, E> {
    pub const fn new(value: T, error: E) -> Self {
        Self { value, error }
    }

    pub const fn value(&self) -> &T {
        &self.value
    }

    pub const fn error(&self) -> &E {
        &self.error
    }

    pub fn into_parts(self) -> (T, E) {
        (self.value, self.error)
    }

    pub fn map<U>(self, map: impl FnOnce(T) -> U) -> Rejected<U, E> {
        Rejected::new(map(self.value), self.error)
    }

    pub fn discard(self) -> E {
        self.error
    }
}

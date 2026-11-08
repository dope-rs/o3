/// An exclusive logical identity projection of an initialized native slot.
pub struct Occupied<B> {
    pub(in crate::depot) inner: B,
}

impl<B> From<B> for Occupied<B> {
    fn from(inner: B) -> Self {
        Self { inner }
    }
}

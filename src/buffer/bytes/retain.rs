use crate::buffer::{bytes, pool, pool::owners, storage};
pub trait Retain {}
impl<R: bytes::TryRetain> Retain for bytes::Source<'_, R> {}
impl Retain for &[u8] {}
impl Retain for storage::Shared {}
impl<L> Retain for storage::Shared<owners::Edge<owners::Plain<L>, pool::Borrowed<'_>>> {}

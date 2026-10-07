use std::marker;

/// A withdrawn value whose original identity retires unless restored.
pub struct Withdrawn<B, Tag = (), const ROOT: bool = true> {
    pub(in crate::depot::arena::slab) entry: B,
    pub(in crate::depot::arena::slab) tag: marker::PhantomData<fn() -> Tag>,
}

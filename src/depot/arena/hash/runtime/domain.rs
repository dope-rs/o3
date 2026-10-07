use std::marker;

/// Keeps resident hash identities distinct from independently managed slab identities.
#[derive(Debug)]
pub struct Domain<Tag = ()>(marker::PhantomData<fn(Tag) -> Tag>);

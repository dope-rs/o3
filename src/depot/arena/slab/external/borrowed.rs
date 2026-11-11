use crate::depot::arena::slab;

/// A logical identity projection of its native borrowed phase.
pub type Borrowed<B, Tag = (), const ROOT: bool = true> =
    super::Occupied<slab::Borrowed<B, Tag, ROOT>>;

const _: () = {
    type Slot =
        Borrowed<slab::entries::Busy<'static, u64, { u32::MAX }, slab::entries::Interior<true>>>;
    assert!(size_of::<Slot>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<Slot>>() == size_of::<Slot>());
};

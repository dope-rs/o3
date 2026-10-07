pub(in crate::buffer) enum Access<'a, P: crate::buffer::raw::Policy = ()> {
    Unique(crate::buffer::raw::Unique<'a, P>),
    Shared,
}

const _: () = assert!(size_of::<Access<'static>>() == size_of::<usize>());

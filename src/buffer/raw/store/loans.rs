use crate::buffer::{self, bytes::bounded, storage::provided};

// SAFETY: mutable access reborrows this exact slice while its slot edge remains in place.
unsafe impl<O> buffer::raw::Mutable for provided::Slice<&mut [u8], O> where Self: buffer::raw::Source
{}

// SAFETY: the prefix's shared access preserves its exact borrowed range and owner.
unsafe impl<T: buffer::raw::Source> buffer::raw::Source for provided::Prefix<'_, '_, T> {}

// SAFETY: Selection removes exactly the admitted prefix from the original slice;
// both disjoint borrows retain the unchanged provided owner and its lifetime.
unsafe impl buffer::raw::Split for provided::Prefix<'_, '_> {}

// SAFETY: the bound forwards shared access to the same stable provided prefix.
unsafe impl<T: buffer::raw::Source> buffer::raw::Source
    for bounded::Value<provided::Prefix<'_, '_, T>>
{
}

// SAFETY: conversion moves the original byte borrow and provided owner unchanged.
unsafe impl<'a, 'd> buffer::raw::Freeze for provided::Prefix<'a, 'd> {
    type Output = provided::Prefix<'a, 'd, &'a [u8]>;

    fn freeze(self) -> Self::Output {
        self.into()
    }
}

// SAFETY: conversion retains the bound and exact prefix borrow and owner unchanged.
unsafe impl<'a, 'd> buffer::raw::Freeze for bounded::Value<provided::Prefix<'a, 'd>> {
    type Output = bounded::Value<provided::Prefix<'a, 'd, &'a [u8]>>;

    fn freeze(self) -> Self::Output {
        self.into()
    }
}

// SAFETY: the bound delegates mutable access to the unchanged provided slice.
unsafe impl<'a, O> buffer::raw::Mutable for bounded::Value<provided::Slice<&'a mut [u8], O>>
where
    provided::Slice<&'a mut [u8], O>: buffer::raw::Mutable,
    Self: buffer::raw::Source,
{
}

// SAFETY: the inner prefix performs the exact split; wrapping it preserves
// both disjoint ranges and their original owner and lifetimes.
unsafe impl buffer::raw::Split for bounded::Value<provided::Prefix<'_, '_>> {}

// SAFETY: roots borrow the original arena and the terminal reclaims every initialized slot and charge.
unsafe impl<T, C> buffer::raw::Arenas for provided::Arena<T, C> {
    type Roots<'d>
        = provided::Root<'d, T, C>
    where
        Self: 'd;
    unsafe fn enter(&mut self) -> (Self::Roots<'_>, impl FnOnce() + '_) {
        unsafe { self.enter_scope() }
    }
}

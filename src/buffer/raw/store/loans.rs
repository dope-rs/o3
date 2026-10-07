use crate::{
    buffer::{
        self,
        bytes::bounded,
        storage::{
            provided,
            provided::{Account, Unique},
        },
    },
    mem::rc,
};

// SAFETY: Unique retains the view's exact exclusive extent and its provided owner.
unsafe impl<'d> buffer::raw::Project<&'d mut [u8]> for Unique<'d> {
    fn as_inner(&self) -> &&'d mut [u8] {
        self.inner()
    }

    unsafe fn as_inner_mut(&mut self) -> &mut &'d mut [u8] {
        unsafe { self.inner_mut() }
    }
}

// SAFETY: view retains the exclusive initialized range paired with this owner.
unsafe impl buffer::raw::Consume for Unique<'_> {
    fn consume<const MAX: u32>(prefix: buffer::raw::Consumed<'_, '_, Self, MAX>) {
        prefix.consume::<&mut [u8]>();
    }
}

// SAFETY: the projection reborrows the same exclusive view without changing its owner.
unsafe impl buffer::raw::Mutable for Unique<'_> {}

// SAFETY: the prefix's shared access preserves its exact borrowed range and owner.
unsafe impl<T: buffer::raw::Source> buffer::raw::Source for provided::Prefix<'_, '_, T> {}

// SAFETY: the mutable prefix projection returns its unchanged exclusive byte slice.
unsafe impl buffer::raw::Mutable for provided::Prefix<'_, '_> {}

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

// SAFETY: the bound forwards the exact unique owner's mutable byte projection.
unsafe impl buffer::raw::Mutable for bounded::Value<Unique<'_>> {}

// SAFETY: the bound forwards the exact prefix owner's mutable byte projection.
unsafe impl buffer::raw::Mutable for bounded::Value<provided::Prefix<'_, '_>> {}

// SAFETY: the inner prefix performs the exact split; wrapping it preserves
// both disjoint ranges and their original owner and lifetimes.
unsafe impl buffer::raw::Split for bounded::Value<provided::Prefix<'_, '_>> {}

// SAFETY: Unique retains its provided owner while the paired slice restricts its extent.
unsafe impl buffer::raw::Store for Unique<'_> {
    fn empty() -> Option<Self> {
        None
    }

    fn restrict(slice: buffer::raw::Slice<'_, '_, Self>) {
        // SAFETY: Unique exposes the same exclusive extent and retains its owner;
        // inner projection preserves the slice's canonical empty-storage policy.
        let slice = unsafe { slice.map(|source| source.inner_mut()) };
        <&mut [u8] as buffer::raw::Store>::project(slice);
    }
}
// SAFETY: the concrete edge retains its original allocation, ownership, and lifetime.
unsafe impl<'d> buffer::raw::Retain for Account<'d> {
    fn try_retain(&self) -> Result<Self, rc::Error> {
        self.try_clone()
    }
}

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

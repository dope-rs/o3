use std::{
    alloc::Layout,
    marker,
    mem::{ManuallyDrop, offset_of, take},
    num, ptr, vec,
};

use storage::backing;

use crate::{
    buffer,
    buffer::{pool, pool::owners, storage, storage::regions},
    mem,
    mem::rc,
};

/// One compact plain backing edge retaining its original root mode.
#[derive(Debug)]
#[repr(transparent)]
pub struct Tag<M = pool::Owned> {
    pointer: ptr::NonNull<()>,
    mode: marker::PhantomData<M>,
}

enum Kind {
    Allocation,
    Vec,
    Pool,
}

impl Tag {
    pub(in crate::buffer) fn allocation(owner: crate::buffer::raw::Rc) -> Self {
        Self {
            pointer: owner.into_raw().cast(),
            mode: marker::PhantomData,
        }
    }

    pub(super) fn vec(owner: mem::raw::Owner<Header>) -> Self {
        Self {
            pointer: owner.into_raw().cast().map_addr(|address| address | 0b010),
            mode: marker::PhantomData,
        }
    }
}

impl<M> Tag<M> {
    pub(in crate::buffer) fn pool<L>(owner: owners::Edge<owners::Plain<L>, M>) -> Option<Self> {
        owner.into_raw().map(|pointer| Self {
            pointer: pointer.cast().map_addr(|address| address | 0b100),
            mode: marker::PhantomData,
        })
    }

    pub(in crate::buffer) fn bytes(&self) -> usize {
        self.root().bytes()
    }

    pub(in crate::buffer) fn root<'a, 'd>(&'a self) -> backing::Root<'a, 'd> {
        use backing::Root;

        let pointer = self.untag();
        match self.kind() {
            // SAFETY: each exact tag borrows its initialized backing for the owner borrow.
            Kind::Allocation => Root::Allocation(unsafe { pointer.cast().as_ref() }),
            Kind::Vec => Root::Vec(unsafe { pointer.cast().as_ref() }),
            Kind::Pool => Root::Pool(unsafe { pointer.cast().as_ref() }),
        }
    }

    fn kind(&self) -> Kind {
        if self.pointer.addr().get() & 0b010 != 0 {
            Kind::Vec
        } else if self.pointer.addr().get() & 0b100 != 0 {
            Kind::Pool
        } else {
            Kind::Allocation
        }
    }

    pub(in crate::buffer) fn untag(&self) -> ptr::NonNull<()> {
        // SAFETY: every constructor retains an aligned, non-null backing address.
        self.pointer
            .map_addr(|address| unsafe { num::NonZero::new_unchecked(address.get() & !7) })
    }

    pub(in crate::buffer) fn try_clone(&self) -> Result<Self, rc::Error> {
        // SAFETY: this owner keeps its exact count live during admission.
        unsafe { mem::raw::Edge::try_retain(self) }?;
        Ok(Self {
            pointer: self.pointer,
            mode: marker::PhantomData,
        })
    }
}

impl Tag<pool::Borrowed<'_>> {
    pub(in crate::buffer) fn into_owned(self) -> Tag {
        let this = ManuallyDrop::new(self);
        Tag {
            pointer: this.pointer,
            mode: marker::PhantomData,
        }
    }
}

impl<M> Drop for Tag<M> {
    fn drop(&mut self) {
        let pointer = self.untag();
        match self.kind() {
            Kind::Vec => {
                // SAFETY: Drop transfers this exact vector edge once.
                unsafe { mem::raw::Owner::<Header>::release_raw(pointer.cast()) };
            }
            Kind::Pool => {
                // SAFETY: Drop transfers this exact plain pool-slot edge once.
                unsafe { owners::Edge::<owners::Plain>::release_raw(pointer) };
            }
            Kind::Allocation => {
                // SAFETY: Drop transfers this exact plain allocation edge once.
                unsafe {
                    mem::raw::Owner::<crate::buffer::raw::Header<()>>::release_raw(pointer.cast())
                };
            }
        }
    }
}

const _: () = {
    assert!(align_of::<crate::buffer::raw::Header<()>>() >= 8);
    assert!(align_of::<regions::Prefix>() >= 8);
    assert!(size_of::<Tag>() == size_of::<usize>());
    assert!(size_of::<Option<Tag>>() == size_of::<usize>());
    assert!(size_of::<Tag<pool::Borrowed<'static>>>() == size_of::<usize>());
    assert!(size_of::<Option<Tag<pool::Borrowed<'static>>>>() == size_of::<usize>());
};

impl<'d> backing::Owner<'d> for Tag<pool::Borrowed<'d>> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        Some(self.root())
    }
}

impl<'d> backing::Owner<'d> for Option<regions::Tag> {
    fn root<'a>(&'a self) -> Option<backing::Root<'a, 'd>>
    where
        'd: 'a,
    {
        self.as_ref().map(regions::Tag::root)
    }
}

impl regions::End for Option<regions::Tag> {
    const EMPTY: Option<Self> = Some(None);
}

impl buffer::Residency for Option<regions::Tag> {
    fn resident(&self) -> usize {
        self.as_ref().map_or(0, regions::Tag::bytes)
    }
}

#[repr(C, align(8))]
pub(in crate::buffer) struct Header {
    refs: mem::raw::Count,
    buf: vec::Vec<u8>,
}

impl Header {
    pub(in crate::buffer) fn try_new<T: AsRef<[u8]> + Default + Into<vec::Vec<u8>>>(
        source: &mut T,
    ) -> Result<regions::Value<Option<regions::Tag>>, crate::alloc::Exhausted> {
        if source.as_ref().is_empty() {
            *source = T::default();
            return Ok(regions::Value::empty(None));
        }
        let pointer =
            crate::alloc::raw::LayoutExt::try_allocate::<Header>(Layout::new::<Header>())?;
        let buf = take(source).into();
        let extent = regions::Extent::from_bytes(buf.as_slice());
        // SAFETY: this control block owns the exact Header layout.
        unsafe {
            pointer.write(Header {
                refs: mem::raw::Count::one(),
                buf,
            })
        };
        // SAFETY: the initialized header transfers its initial counted edge.
        let owner = Some(regions::Tag::vec(unsafe {
            mem::raw::Owner::from_raw(pointer)
        }));
        // SAFETY: the vector owner retains this exact initialized extent.
        Ok(unsafe { regions::Value::new(owner, extent) })
    }

    pub(in crate::buffer) fn capacity(&self) -> usize {
        self.buf.capacity()
    }
}

// SAFETY: each vector header owns exactly this reference domain.
unsafe impl mem::raw::Edge for Header {
    fn count(&self) -> &mem::raw::Count {
        &self.refs
    }
}

// SAFETY: the count and vector belong to this exact allocated control block.
unsafe impl mem::raw::Target for Header {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: Last transfers the initialized Header and its exact global allocation.
        drop(unsafe { Box::from_raw(pointer.as_ptr()) });
    }
}

const _: () = {
    assert!(size_of::<Header>() == 4 * size_of::<usize>());
    assert!(align_of::<Header>() >= 8);
    assert!(offset_of!(Header, refs) == 0);
};

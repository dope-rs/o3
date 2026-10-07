use std::{alloc::Layout, marker, mem};

use crate::alloc::{self, tail};

/// Validated layout of one header followed by an initialized tail.
#[repr(transparent)]
pub struct Plan<H, T, N: alloc::raw::TailExtent = usize> {
    value: N,
    types: marker::PhantomData<fn() -> (H, T)>,
}

/// Header followed by one exact initialized tail.
#[repr(C)]
pub(in crate::alloc::tail) struct Prefix<H, T, N: alloc::raw::TailExtent = usize> {
    pub(in crate::alloc::tail) header: H,
    pub(in crate::alloc::tail) len: Plan<H, T, N>,
}

impl<H, T, N: alloc::raw::TailExtent> Plan<H, T, N> {
    pub(in crate::alloc) const fn tail_offset() -> usize {
        // SAFETY: callers retain admission of this exact header-and-tail layout.
        unsafe { alloc::raw::Layout::<T>::offset(mem::size_of::<Prefix<H, T, N>>()) }
    }

    pub fn try_new(extent: N) -> Result<Self, alloc::Overflow> {
        if !Self::fits(extent.len()) {
            return Err(alloc::Overflow);
        }
        Ok(Self {
            value: extent,
            types: marker::PhantomData,
        })
    }

    /// Allocates this exact layout without initializing its header or tail.
    pub fn try_allocate(self) -> Result<tail::Uninit<H, T, N>, alloc::Exhausted> {
        let memory = alloc::raw::LayoutExt::try_allocate(self.layout())?;
        Ok(tail::Uninit {
            memory,
            layout: self,
        })
    }

    const fn fits(len: usize) -> bool {
        alloc::raw::Layout::<T>::fits_after(Layout::new::<Prefix<H, T, N>>(), len)
    }

    /// The exact admitted allocation layout, including its retained extent and padding.
    pub fn layout(&self) -> Layout {
        // SAFETY: Plan retains validation of this exact header-and-tail layout.
        unsafe { Self::layout_for(self.len()) }
    }

    /// # Safety
    /// `len` has passed this plan's header-and-tail layout check.
    const unsafe fn layout_for(len: usize) -> Layout {
        // SAFETY: admission checked both the tail extent and padded total size.
        unsafe { alloc::raw::Layout::<T>::extend(Layout::new::<Prefix<H, T, N>>(), len) }
            .0
            .pad_to_align()
    }

    pub(in crate::alloc::tail) fn len(self) -> usize {
        self.value.len()
    }

    pub(in crate::alloc::tail) const fn extent(self) -> N {
        self.value
    }
}

impl<H, T> Plan<H, T> {
    /// Exact allocation bytes, including the header and alignment padding.
    pub const fn size(self) -> usize {
        // SAFETY: the stored extent retains this exact layout's admission check.
        unsafe { Self::layout_for(self.value) }.size()
    }

    #[must_use]
    pub const fn fixed<const LEN: usize>() -> Self {
        const {
            assert!(Self::fits(LEN), "layout");
        };
        Self {
            value: LEN,
            types: marker::PhantomData,
        }
    }
}

impl<H, T, N: alloc::raw::TailExtent> Clone for Plan<H, T, N> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<H, T, N: alloc::raw::TailExtent> Copy for Plan<H, T, N> {}

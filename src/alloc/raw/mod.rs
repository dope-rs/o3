use std::{alloc, marker, mem, num::NonZero, pin, ptr};

pub use block::Block;
pub use extent::Extent;
pub(crate) use layout::Bounds;
pub use layout::Layout;
pub use rows::Rows;
pub use shape::Shape;

use crate::{
    alloc::{Exhausted, Overflow, Vacant, array},
    base::{cell::region, marker::ThreadBound},
};
mod layout;
pub(crate) use layout::Root;
pub use resident::Resident;

/// Initialization proof for an auxiliary plane reconstructed from its shape.
#[derive(Debug)]
pub struct Initialized;

mod block;
mod extent;
mod resident;
mod rows;
mod shape;

pub trait VecExt<T> {
    /// Retains one previously admitted insertion.
    /// # Safety
    /// This vector's length is strictly below its capacity.
    unsafe fn vacancy(&mut self) -> Vacant<'_, T>;

    /// Removes one retained resident and returns the tail moved into its place.
    /// # Safety
    /// `index` is below this exclusively borrowed vector's current length.
    unsafe fn swap_remove(&mut self, index: usize) -> (T, Option<&T>);
}

impl<T> VecExt<T> for Vec<T> {
    unsafe fn vacancy(&mut self) -> Vacant<'_, T> {
        Vacant {
            values: self,
            _bound: ThreadBound::NEW,
        }
    }

    unsafe fn swap_remove(&mut self, index: usize) -> (T, Option<&T>) {
        let last = self.len() - 1;
        let pointer = self.as_mut_ptr();
        // SAFETY: the retained resident proves nonemptiness; ownership leaves the tail once.
        let value = unsafe { pointer.add(last).read() };
        unsafe { self.set_len(last) };
        if index == last {
            return (value, None);
        }
        // SAFETY: the distinct admitted index remains initialized in the shortened vector.
        let slot = unsafe { &mut *pointer.add(index) };
        (mem::replace(slot, value), Some(slot))
    }
}

/// Transfers a completely initialized backing into its element owner.
pub trait AssumeInit: Sized {
    type Value;
    type Output;

    /// # Safety
    /// Every element is initialized, and the block exclusively owns its drop obligation.
    unsafe fn assume_init(self) -> Self::Output;

    /// # Safety
    /// The retained source yields at most this allocation's extent and exactly it on natural completion.
    /// Its count, order, ownership and item borrows stay valid through callbacks and cleanup.
    unsafe fn try_map<I: Iterator, F>(
        self,
        source: I,
        make: impl for<'slot> FnMut(
            array::Slot<'slot, Self::Value>,
            I::Item,
        ) -> Result<array::Written<'slot>, F>,
    ) -> Result<Self::Output, F>;
}

/// Relocates admitted storage runs into their replacement, retaining the source state.
/// # Safety
/// Nonempty backing allocations are disjoint; every run is bounded by their extents.
/// Spare has no outstanding element borrows or destruction obligations.
/// Runs transfer all owned initialization exactly once to disjoint target positions.
/// Restore the owner's metadata to those positions before allowing it to be reused.
pub unsafe fn relocate<T, E: Extent, S, const N: usize>(
    source: &mut array::Block<mem::MaybeUninit<T>, E, S>,
    mut spare: array::Block<mem::MaybeUninit<T>, E>,
    runs: [(usize, usize, usize); N],
) {
    for (start, target, len) in runs {
        if len != 0 && mem::size_of::<T>() != 0 {
            // SAFETY: the retained relocation admits these complete disjoint runs.
            unsafe {
                ptr::copy_nonoverlapping(
                    source.as_mut_ptr().add(start),
                    spare.as_mut_ptr().add(target),
                    len,
                );
            }
        }
    }
    source.swap_storage(&mut spare);
}

/// Proof that an extent describes a zero-length allocation.
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Empty<E: Extent>(E);

impl<E: Extent> Empty<E> {
    /// # Safety
    /// `extent` must report zero through its `Extent` implementation.
    pub const unsafe fn assume(extent: E) -> Self {
        Self(extent)
    }

    pub(in crate::alloc) const fn into_inner(self) -> E {
        self.0
    }
}

/// Exact stored extent and its canonical zero-based index domain.
///
/// # Safety
/// Copies retain immutable `len`; `indices` yields `0..len` exactly once in order.
pub unsafe trait TailExtent: Extent {
    #[doc(hidden)]
    type Index: TailIndex;

    #[doc(hidden)]
    fn indices(self) -> impl Iterator<Item = Self::Index>;
}

// SAFETY: `0..self` yields exactly `self` ordered indices.
unsafe impl TailExtent for usize {
    type Index = usize;

    fn indices(self) -> impl Iterator<Item = Self::Index> {
        0..self
    }
}

// SAFETY: the nonzero count is retained; the range yields every u32 index once.
unsafe impl TailExtent for NonZero<u32> {
    type Index = u32;

    fn indices(self) -> impl Iterator<Item = Self::Index> {
        0..self.get()
    }
}

/// An immutable zero-based storage offset.
///
/// # Safety
/// Every copy must retain the same exact offset without truncation.
pub unsafe trait TailIndex: Copy {
    #[doc(hidden)]
    fn get(self) -> usize;
}

unsafe impl TailIndex for usize {
    fn get(self) -> usize {
        self
    }
}

unsafe impl TailIndex for u32 {
    fn get(self) -> usize {
        const { assert!(usize::BITS >= u32::BITS) };
        self as usize
    }
}

impl<T, E: Extent, S> AssumeInit for array::Block<mem::MaybeUninit<T>, E, S> {
    type Value = T;
    type Output = array::Block<T, E, S>;

    unsafe fn assume_init(self) -> Self::Output {
        // SAFETY: the caller transfers the complete initialized backing.
        unsafe { self.assume_init() }
    }

    unsafe fn try_map<I: Iterator, F>(
        mut self,
        source: I,
        mut make: impl for<'slot> FnMut(
            array::Slot<'slot, T>,
            I::Item,
        ) -> Result<array::Written<'slot>, F>,
    ) -> Result<array::Block<T, E, S>, F> {
        let pointer = ptr::NonNull::from(&mut *self).cast::<T>();
        let mut filled = 0;
        for item in source {
            // SAFETY: the retained exact source supplies one item for each admitted
            // slot, in order; this slot is still unpublished and uninitialized.
            let place = unsafe { pointer.add(filled) };
            let status = region::Token::scope_pinned(pin::Pin::new(&mut filled), |token, count| {
                make(
                    array::Slot {
                        pointer: place,
                        filled: count.get_mut(),
                        proof: token.proof(),
                        value: marker::PhantomData,
                    },
                    item,
                )
                .map(|done| *token.borrow(&done.value))
            });
            if let Err(error) = status {
                // SAFETY: this prefix includes every committed value, including the
                // current one when the callback failed after insertion. The block still
                // owns the backing as MaybeUninit<T> and releases it after make.
                unsafe {
                    ptr::drop_in_place(ptr::slice_from_raw_parts_mut(pointer.as_ptr(), filled));
                }
                return Err(error);
            }
        }
        // SAFETY: terminal iteration completed exactly the retained extent.
        Ok(unsafe { self.assume_init() })
    }
}

pub(in crate::alloc) fn into_vec<T>(block: array::Block<T, usize, ()>) -> Vec<T> {
    let mut block = mem::ManuallyDrop::new(block);
    let len = *block.extent();
    let pointer = block.as_mut_ptr();
    // SAFETY: the consumed block owns exactly len initialized T values in
    // the standard allocator's matching array layout. Its unit state has
    // no drop obligation; empty and zero-sized arrays retain aligned storage.
    unsafe { Vec::from_raw_parts(pointer, len, len) }
}

#[repr(C)]
pub struct Tail<S, T> {
    prior: S,
    offset: usize,
    value: marker::PhantomData<fn() -> T>,
}

pub trait Recipe: Sized {
    type Fields;

    fn try_build(self) -> Result<Bundle<Self::Fields>, Overflow>;

    /// # Safety
    /// A nominal proof must establish that this exact field sequence fits.
    unsafe fn build(self) -> Bundle<Self::Fields>;

    fn tail<T>(self, len: usize) -> impl Recipe<Fields = Tail<Self::Fields, T>> {
        layout::Tail {
            prior: self,
            len,
            value: marker::PhantomData,
        }
    }
}

/// Global allocation operations for one exact standard layout.
pub trait LayoutExt: Sized {
    /// Allocates uninitialized storage, or an aligned dangling pointer for an empty layout.
    /// The pointer's type does not establish initialization, size, or alignment for `T`.
    fn try_allocate<T>(self) -> Result<ptr::NonNull<T>, Exhausted>;

    /// # Safety
    /// `pointer` transfers a live allocation made with this exact layout.
    /// All contained values and borrows must have ended. Empty layouts need no allocation.
    unsafe fn deallocate<T>(self, pointer: ptr::NonNull<T>);
}

impl LayoutExt for alloc::Layout {
    fn try_allocate<T>(self) -> Result<ptr::NonNull<T>, Exhausted> {
        if self.size() == 0 {
            return Ok(self.dangling_ptr().cast());
        }
        // SAFETY: the layout is valid and its zero-sized case returned above.
        unsafe { try_new(self, false) }
    }

    unsafe fn deallocate<T>(self, pointer: ptr::NonNull<T>) {
        if self.size() != 0 {
            // SAFETY: the caller retains the allocation and this layout is nonzero.
            unsafe { dealloc(self, pointer) };
        }
    }
}

/// # Safety
/// This validated layout has nonzero size; zeroed selects initialization only.
pub(crate) unsafe fn try_new<T>(
    layout: alloc::Layout,
    zeroed: bool,
) -> Result<ptr::NonNull<T>, Exhausted> {
    // SAFETY: the caller proves this exact validated layout is nonzero.
    let pointer = unsafe {
        if zeroed {
            alloc::alloc_zeroed(layout)
        } else {
            alloc::alloc(layout)
        }
    };
    ptr::NonNull::new(pointer.cast()).ok_or(Exhausted)
}

/// # Safety
/// `pointer` identifies a live allocation made with this exact nonzero layout.
pub(crate) unsafe fn dealloc<T>(layout: alloc::Layout, pointer: ptr::NonNull<T>) {
    // SAFETY: the caller transfers the original nonzero pointer-layout pair.
    unsafe { alloc::dealloc(pointer.cast().as_ptr(), layout) };
}

/// # Safety
/// `pointer` identifies a live nonzero allocation made with `layout`;
/// `target` is nonzero and has the same alignment.
pub(crate) unsafe fn resize(
    layout: alloc::Layout,
    pointer: ptr::NonNull<u8>,
    target: alloc::Layout,
) -> Result<ptr::NonNull<u8>, Exhausted> {
    // SAFETY: the caller retains the exact allocation and proves a nonzero
    // target with unchanged alignment. Failure leaves the old allocation live.
    let pointer = unsafe { alloc::realloc(pointer.as_ptr(), layout, target.size()) };
    ptr::NonNull::new(pointer).ok_or(Exhausted)
}

pub(in crate::alloc) fn init<T>(
    entries: &mut [mem::MaybeUninit<T>],
    mut make: impl FnMut(usize) -> T,
) {
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.write(make(index));
    }
}

/// Writes paired rows in order; the caller retains each plane's complete
/// extent and initialization proof, including an uninitialized first plane.
pub(in crate::alloc) fn pair<T, U>(
    entries: &mut [mem::MaybeUninit<T>],
    other: &mut [mem::MaybeUninit<U>],
    make: impl Fn(usize) -> (mem::MaybeUninit<T>, U),
) {
    for (index, (first, second)) in entries.iter_mut().zip(other).enumerate() {
        let (left, right) = make(index);
        *first = left;
        second.write(right);
    }
}

#[repr(C)]
pub struct Bundle<S> {
    layout: alloc::Layout,
    fields: S,
}

impl Bundle<()> {
    /// Checks the complete field sequence declared by a Shape without creating
    /// an instance that would already claim its allocation is representable.
    pub const fn try_layout<S: Shape + ?Sized>(
        len: usize,
        tail_len: usize,
    ) -> Result<alloc::Layout, Overflow> {
        let bounds = Self::bounds::<S>(len, tail_len);
        if !bounds.fits() {
            return Err(Overflow);
        }
        // SAFETY: fits admits the complete retained four-plane extent.
        Ok(unsafe { bounds.layout() }.pad_to_align())
    }

    pub(crate) const fn bounds<S: Shape + ?Sized>(len: usize, tail_len: usize) -> Bounds {
        Bounds::new(alloc::Layout::new::<()>())
            .extend::<S::Root>(len)
            .extend::<S::Tail>(tail_len)
            .extend::<S::Extra>(len)
            .extend::<S::Last>(len)
    }

    pub(crate) fn try_root<T>(len: usize) -> Result<Bundle<Layout<T>>, Overflow> {
        Ok(Bundle {
            layout: Layout::<T>::try_layout(len)?,
            fields: Layout(marker::PhantomData),
        })
    }

    /// # Safety
    /// The caller carries a nominal proof that this exact array layout fits.
    pub(crate) const unsafe fn root<T>(len: usize) -> Bundle<Layout<T>> {
        Bundle {
            layout: unsafe { Layout::<T>::layout(len) },
            fields: Layout(marker::PhantomData),
        }
    }
}

impl<S> Bundle<S> {
    pub(crate) const fn layout(&self) -> alloc::Layout {
        self.layout.pad_to_align()
    }

    /// Allocates these exact fields and retains their layout for raw projection.
    pub fn try_allocate(&self) -> Result<Fields<'_, S>, Exhausted> {
        Ok(Fields {
            fields: &self.fields,
            pointer: LayoutExt::try_allocate(self.layout())?,
        })
    }

    /// # Safety
    /// `pointer` identifies a live allocation made with this exact bundle.
    /// No owner or field projection may access the allocation after this call.
    pub unsafe fn deallocate(&self, pointer: ptr::NonNull<u8>) {
        unsafe { LayoutExt::deallocate(self.layout(), pointer) }
    }
}

impl<S> Bundle<S> {
    pub(crate) fn try_tail<T>(self, len: usize) -> Result<Bundle<Tail<S, T>>, Overflow> {
        let (layout, offset) = Layout::<T>::try_extend(self.layout, len)?;
        Ok(Bundle {
            layout,
            fields: Tail {
                prior: self.fields,
                offset,
                value: marker::PhantomData,
            },
        })
    }

    /// # Safety
    /// The caller carries a nominal proof that this exact extension fits.
    pub(crate) unsafe fn tail<T>(self, len: usize) -> Bundle<Tail<S, T>> {
        // SAFETY: the nominal proof covers this exact array extension.
        let (layout, offset) = unsafe { Layout::<T>::extend(self.layout, len) };
        Bundle {
            layout,
            fields: Tail {
                prior: self.fields,
                offset,
                value: marker::PhantomData,
            },
        }
    }
}

pub struct Fields<'a, S> {
    fields: &'a S,
    pointer: ptr::NonNull<u8>,
}

impl<T> Fields<'_, Layout<T>> {
    pub(crate) const fn root(self) -> ptr::NonNull<T> {
        self.pointer.cast()
    }
}

impl<'a, S, T> Fields<'a, Tail<S, T>> {
    pub fn tail(self) -> ptr::NonNull<T> {
        // SAFETY: this allocation was made from the retained field sequence;
        // its admitted offset stays within the allocation and is aligned for T.
        unsafe { self.pointer.add(self.fields.offset) }.cast()
    }

    pub const fn prior(self) -> Fields<'a, S> {
        Fields {
            fields: &self.fields.prior,
            pointer: self.pointer,
        }
    }
}

impl<S> Clone for Fields<'_, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<S> Copy for Fields<'_, S> {}

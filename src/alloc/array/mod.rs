use std::{iter, marker, mem, ops, ptr, slice};

use crate::{
    alloc::{self, array},
    base::cell::region,
};

mod plan;
pub use plan::Plan;

/// Owns a completely initialized array and its exact allocation extent.
#[must_use]
#[derive(Debug)]
#[repr(C)]
pub struct Block<T, E: alloc::raw::Extent, S = ()> {
    pub(in crate::alloc::array) plan: array::Plan<T, E>,
    pub(in crate::alloc::array) state: S,
    pub(in crate::alloc::array) pointer: ptr::NonNull<T>,
}

/// One admitted final position and its exclusively borrowed initialized prefix.
#[must_use]
#[derive(Debug)]
#[repr(C)]
pub struct Slot<'slot, T> {
    pub(in crate::alloc) pointer: ptr::NonNull<T>,
    pub(in crate::alloc) filled: &'slot mut usize,
    pub(in crate::alloc) proof: region::Proof<region::Identity<'slot>>,
    pub(in crate::alloc) value: marker::PhantomData<fn(T) -> T>,
}

/// Completion of the single insertion admitted by this invariant scope.
#[must_use]
#[derive(Debug)]
#[repr(transparent)]
pub struct Written<'slot> {
    pub(in crate::alloc) value: region::Value<region::Identity<'slot>, ()>,
}

impl<'slot, T> Slot<'slot, T> {
    /// Initializes the admitted position and transfers destruction to its prefix.
    /// Admission keeps `filled` below the retained extent, so the single consuming
    /// insertion cannot overflow the initialized prefix.
    pub fn insert(self, value: T) -> Written<'slot> {
        // SAFETY: private admission retains this exact uninitialized position
        // and its backing throughout the invariant callback scope.
        unsafe { self.pointer.write(value) };
        *self.filled += 1;
        Written {
            value: self.proof.bind(()),
        }
    }
}

impl<T, E: alloc::raw::Extent, S> array::Block<mem::MaybeUninit<T>, E, S> {
    pub fn try_new(extent: E, state: S) -> Result<Self, alloc::Error> {
        Ok(array::Plan::try_new(extent)?.try_allocate(state)?)
    }

    /// Initializes the entire unpublished backing and transfers its ownership.
    pub fn init(mut self, make: impl FnMut(usize) -> T) -> array::Block<T, E, S> {
        alloc::raw::init(&mut self, make);
        // SAFETY: the complete owned extent now contains initialized elements.
        unsafe { self.assume_init() }
    }

    /// Initializes each element at its final address. On failure, drops the
    /// completed prefix and releases the backing and state.
    pub fn try_init<F>(
        self,
        mut make: impl FnMut(ptr::NonNull<T>) -> Result<T, F>,
    ) -> Result<array::Block<T, E, S>, F> {
        self.try_fill(move |slot| {
            let value = make(slot.pointer)?;
            Ok(slot.insert(value))
        })
    }

    /// Initializes each final slot through one consuming write capability.
    /// An error releases the committed prefix, factory, backing, and state.
    pub fn try_fill<F>(
        self,
        mut make: impl for<'slot> FnMut(Slot<'slot, T>) -> Result<Written<'slot>, F>,
    ) -> Result<array::Block<T, E, S>, F> {
        let source = iter::repeat_n((), self.plan.extent().len());
        // SAFETY: this source yields exactly the retained allocation's extent.
        unsafe { alloc::raw::AssumeInit::try_map(self, source, move |slot, ()| make(slot)) }
    }

    /// Initializes every element without requiring its storage index.
    pub fn repeat_with(self, mut make: impl FnMut() -> T) -> array::Block<T, E, S> {
        self.init(|_| make())
    }
}

impl<T, E: alloc::raw::Extent, S> Block<T, E, S> {
    pub const fn empty(extent: alloc::raw::Empty<E>, state: S) -> Self {
        Self {
            plan: array::Plan::empty(extent),
            state,
            pointer: ptr::NonNull::dangling(),
        }
    }

    pub const fn extent(&self) -> &E {
        self.plan.extent()
    }

    pub const fn state(&self) -> &S {
        &self.state
    }

    pub fn state_mut(&mut self) -> &mut S {
        &mut self.state
    }

    pub const fn as_ptr(&self) -> *const T {
        self.pointer.as_ptr()
    }

    pub fn as_mut_ptr(&mut self) -> *mut T {
        self.pointer.as_ptr()
    }

    /// Exclusively borrows the state and its disjoint initialized backing.
    pub fn parts_mut(&mut self) -> (&mut S, &mut [T]) {
        // SAFETY: admission proves this exact array layout, and construction
        // initializes every T; the backing is disjoint from the inline state.
        let values =
            unsafe { slice::from_raw_parts_mut(self.pointer.as_ptr(), self.plan.extent().len()) };
        (&mut self.state, values)
    }
}

impl<T> Block<T, usize, ()> {
    /// Clones an exact slice in order through a fallible element factory.
    pub fn try_clone(
        source: &[T],
        mut make: impl FnMut(&T) -> Result<T, alloc::Exhausted>,
    ) -> Result<Self, alloc::Exhausted> {
        let block = array::Plan::from_slice(source).try_allocate(())?;
        // SAFETY: the retained slice supplies exactly the same element type,
        // count and order as the allocation planned from this source.
        unsafe {
            alloc::raw::AssumeInit::try_map(block, source.iter(), move |slot, item| {
                Ok(slot.insert(make(item)?))
            })
        }
    }

    /// Transfers the initialized allocation without copying its elements.
    pub fn into_vec(self) -> Vec<T> {
        alloc::raw::into_vec(self)
    }
}

impl<T, E: alloc::raw::Extent, S> ops::Deref for Block<T, E, S> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        // SAFETY: construction proves that every element is initialized and
        // owned under this exact immutable extent and validated layout.
        unsafe { slice::from_raw_parts(self.pointer.as_ptr(), self.plan.extent().len()) }
    }
}

impl<T, E: alloc::raw::Extent, S> ops::DerefMut for Block<T, E, S> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.parts_mut().1
    }
}

impl<T, E: alloc::raw::Extent, S> Drop for Block<T, E, S> {
    fn drop(&mut self) {
        // SAFETY: this owner retains every initialized element and its exact layout.
        unsafe {
            ptr::drop_in_place(ptr::slice_from_raw_parts_mut(
                self.pointer.as_ptr(),
                self.plan.extent().len(),
            ));
            alloc::raw::LayoutExt::deallocate(self.plan.layout(), self.pointer);
        }
    }
}

impl<T, E: alloc::raw::Extent, S> array::Block<mem::MaybeUninit<T>, E, S> {
    /// # Safety
    /// Every element is initialized and this owner retains its exact drop obligation.
    pub(in crate::alloc) unsafe fn assume_init(self) -> array::Block<T, E, S> {
        let block = mem::ManuallyDrop::new(self);
        array::Block {
            plan: block.plan.initialized(),
            // SAFETY: the consumed owner transfers its state once without running Drop.
            state: unsafe { ptr::read(&block.state) },
            pointer: block.pointer.cast(),
        }
    }
}

impl<T, E: alloc::raw::Extent, S> Block<mem::MaybeUninit<T>, E, S> {
    pub(in crate::alloc) fn swap_storage(&mut self, other: &mut Block<mem::MaybeUninit<T>, E>) {
        mem::swap(&mut self.plan, &mut other.plan);
        mem::swap(&mut self.pointer, &mut other.pointer);
    }
}

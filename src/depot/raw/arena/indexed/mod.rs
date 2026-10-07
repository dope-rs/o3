use std::{cell::Cell, marker, mem, ptr, slice};

pub(in crate::depot) use layouts::{IndexedMeta, IndexedState};

use crate::depot::{
    arena::slab::key,
    capacity,
    queue::{indexed::slab, links, tagged},
};

mod entries;
mod layouts;

const END: u32 = links::Index::END;
const EMPTY: IndexedState = IndexedState {
    free: END,
    initialized: 0,
    len: 0,
    available: 0,
};

mod hook;
pub(in crate::depot) use hook::IndexedHook;

// SAFETY: raw hooks encode vacancy by their own predecessor index.
unsafe impl IndexedHook for links::Hook {
    type Head = links::Chain<()>;
    type Chain = links::Chain<()>;
    fn new(index: links::Index, next: Option<links::Index>) -> Self {
        Self {
            previous: Cell::new(index.raw()),
            next: Cell::new(links::Index::encode(next)),
        }
    }
    fn is_linked(&self, index: links::Index) -> bool {
        self.previous.get() != index.raw()
    }
    unsafe fn next(&self) -> u32 {
        self.next.get()
    }
}

// SAFETY: tagged hooks distinguish detached indices from admitted lane endpoints.
unsafe impl IndexedHook for tagged::Hook {
    type Head = links::Chain;
    type Chain = ();
    fn new(index: links::Index, next: Option<links::Index>) -> Self {
        Self::free(index, next)
    }
    fn is_linked(&self, index: links::Index) -> bool {
        self.is_linked(index)
    }
    unsafe fn next(&self) -> u32 {
        unsafe { tagged::Hook::next(self) }
    }
}

pub(in crate::depot) struct IndexedBlock<
    T,
    Tag,
    const MAX: u32,
    H: IndexedHook = links::Hook,
    const N: usize = 0,
    C: crate::depot::raw::Count = capacity::Count,
> {
    allocation: ptr::NonNull<IndexedState>,
    pub(in crate::depot) metadata: ptr::NonNull<IndexedMeta<MAX, H>>,
    pub(in crate::depot) shape: tagged::Domain<[H::Head; N], C>,
    pub(in crate::depot) generation: key::Limit<MAX>,
    marker: marker::PhantomData<(*mut Tag, T)>,
}

impl<T, Tag, const MAX: u32, H: IndexedHook, const N: usize, C: crate::depot::raw::Count>
    IndexedBlock<T, Tag, MAX, H, N, C>
{
    pub(in crate::depot) fn try_new(
        shape: tagged::Domain<[H::Head; N], C>,
    ) -> Result<Self, crate::alloc::Error> {
        let plan = crate::alloc::raw::Recipe::try_build(IndexedState::recipe::<T, MAX, H, N>(
            shape.capacity().count().get(),
        ))?;
        if shape.capacity().count().get() == 0 && N == 0 {
            return Ok(Self {
                allocation: ptr::NonNull::dangling(),
                metadata: ptr::NonNull::dangling(),
                shape,
                generation: key::Limit::NEW,
                marker: marker::PhantomData,
            });
        }
        let fields = plan.try_allocate()?;
        let state = fields.prior().prior().prior().root();
        let metadata = fields.prior().tail();
        let mut heads = fields.tail().cast::<[mem::MaybeUninit<H::Head>; N]>();
        let block = Self {
            allocation: state,
            metadata,
            shape,
            generation: key::Limit::NEW,
            marker: marker::PhantomData,
        };
        // SAFETY: allocation uniquely owns the exact uninitialized IndexedState,
        // value, IndexedMeta, and lane-head regions described by plan.
        unsafe {
            state.as_ptr().write(IndexedState {
                free: END,
                initialized: 0,
                len: 0,
                available: shape.capacity().count().raw(),
            });
            for head in heads.as_mut() {
                head.write(H::Head::default());
            }
        }
        Ok(block)
    }

    pub(in crate::depot) fn state(&self) -> &IndexedState {
        if self.capacity() == 0 && N == 0 {
            return &EMPTY;
        }
        // SAFETY: every allocated IndexedBlock initializes IndexedState before publication.
        unsafe { self.allocation.as_ref() }
    }

    pub(in crate::depot) unsafe fn state_mut(&mut self) -> &mut IndexedState {
        unsafe { self.allocation.as_mut() }
    }

    pub(in crate::depot) fn heads(&self) -> &[H::Head; N] {
        // SAFETY: the initialized N-head array follows the allocation's exact metadata region.
        unsafe {
            &*self
                .metadata
                .as_ptr()
                .add(self.capacity())
                .cast::<[H::Head; N]>()
        }
    }

    pub(in crate::depot) fn capacity(&self) -> usize {
        self.shape.capacity().count().get()
    }

    /// # Safety
    /// This block retains an allocated region, excluding the empty zero-lane case.
    pub(in crate::depot) unsafe fn values(&self) -> ptr::NonNull<mem::MaybeUninit<T>> {
        unsafe { self.allocation.byte_add(IndexedState::values::<T>()) }.cast()
    }

    pub(in crate::depot) unsafe fn pair(
        &self,
        index: links::Index,
    ) -> (slab::Handle<Tag, MAX>, &T) {
        let meta = unsafe { self.metadata.add(index.get()).as_ref() };
        let values = unsafe { self.values() };
        let value = unsafe { values.add(index.get()).as_ref().assume_init_ref() };
        (slab::Handle::new(index, meta.generation), value)
    }
}

// SAFETY: IndexedBlock owns one stable initialized IndexedMeta prefix; safe and unchecked
// lookup resolve the same embedded hook for every index in that prefix.
unsafe impl<T, Tag, const MAX: u32, C: crate::depot::raw::Count> crate::depot::raw::Hooks
    for IndexedBlock<T, Tag, MAX, links::Hook, 0, C>
{
    unsafe fn hook_raw(&self, index: links::Index) -> &'_ links::Hook {
        // SAFETY: HookStore's caller proves index is in the initialized IndexedMeta prefix.
        &unsafe { self.metadata.add(index.raw() as usize).as_ref() }.hook
    }
}

impl<T, Tag, const MAX: u32, H: IndexedHook, const N: usize, C: crate::depot::raw::Count> Drop
    for IndexedBlock<T, Tag, MAX, H, N, C>
{
    fn drop(&mut self) {
        if self.capacity() == 0 && N == 0 {
            return;
        }
        let metadata = unsafe {
            slice::from_raw_parts(self.metadata.as_ptr(), self.state().initialized as usize)
        };
        let values = unsafe { self.values() };
        for (index, meta) in metadata.iter().enumerate() {
            // SAFETY: every visited metadata index belongs to the initialized prefix.
            if meta
                .hook
                .is_linked(unsafe { links::Index::new_raw(index as u32) })
            {
                // SAFETY: non-vacancy marks an initialized live value; each
                // initialized index is visited exactly once during terminal drop.
                unsafe { values.add(index).as_mut().assume_init_drop() };
            }
        }
        // SAFETY: this owner retains the capacity admitted by its allocation recipe.
        let plan = unsafe {
            crate::alloc::raw::Recipe::build(IndexedState::recipe::<T, MAX, H, N>(self.capacity()))
        };
        unsafe { plan.deallocate(self.allocation.cast()) };
    }
}

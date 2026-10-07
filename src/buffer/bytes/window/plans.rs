use std::{mem, num, ops, ptr};

use storage::alloc;

use crate::{
    buffer,
    buffer::{bytes::window, pool::owners, resident, resident::staging, storage},
    mem::credit,
};

/// Admitted growth and initialized sources bound to their unique allocation.
pub(in crate::buffer) struct Resize<
    'a,
    P: crate::buffer::raw::Policy,
    const MAX: usize,
    I = &'a [u8],
> {
    source: crate::buffer::raw::Unique<'a, P>,
    ideal: window::Growth<MAX>,
    need: window::Growth<MAX>,
    range: ops::Range<u32>,
    tail: I,
}

impl<'a, P: crate::buffer::raw::Policy, const MAX: usize, I: buffer::raw::SliceExt>
    Resize<'a, P, MAX, I>
{
    /// Grows the admitted owner, retrying once with the required capacity.
    fn try_commit(self) -> Result<window::Growth<MAX>, alloc::Error<P::Failure>> {
        let Resize {
            mut source,
            ideal,
            need,
            range,
            tail,
        } = self;
        let at_start = range.start == 0;
        let mut growth = ideal;
        loop {
            let target = growth.capacity().nonzero();
            let amount = growth.amount();
            let error = {
                let debit =
                    P::try_acquire(crate::buffer::raw::Debit::grow(source.policy(), amount))
                        .map_err(alloc::Error::Policy)?;
                if at_start {
                    match source.try_resize(target, debit) {
                        Ok(()) => {
                            // SAFETY: Resize admitted this disjoint tail after the
                            // initialized prefix; resizing preserves that prefix.
                            unsafe {
                                tail.copy_to(source.data().cast_mut().add(range.end as usize));
                            }
                            return Ok(growth);
                        }
                        Err(error) => error,
                    }
                } else {
                    let plan = crate::buffer::raw::Header::<P>::layout(target);
                    // SAFETY: this admitted header includes a nonzero target capacity.
                    match unsafe { crate::alloc::raw::try_new(plan, false) } {
                        Ok(pointer) => {
                            source.replace(pointer, target, debit, range, tail);
                            return Ok(growth);
                        }
                        Err(error) => error,
                    }
                }
            };
            if growth.capacity() == need.capacity() {
                return Err(alloc::Error::Allocation(error));
            }
            growth = need;
        }
    }
}

pub(in crate::buffer) struct Edit<'a, P: crate::buffer::raw::Policy, const MAX_CAPACITY: usize> {
    pub(in crate::buffer::bytes::window) state: &'a mut window::State<P, MAX_CAPACITY>,
    pub(in crate::buffer::bytes::window) tail: &'a mut u32,
}

impl<'a, P: buffer::raw::Policy, const MAX_CAPACITY: usize> Edit<'a, P, MAX_CAPACITY> {
    fn try_full<I: buffer::raw::SliceExt>(
        self,
        src: I,
        required: window::Capacity<MAX_CAPACITY>,
        ideal: window::Capacity<MAX_CAPACITY>,
    ) -> Result<(), alloc::Error<P::Failure>> {
        let Some(growth) = window::Growth::new(self.state.cap, required) else {
            if self.state.buf.is_unique() {
                buffer::raw::Copy::copy(Append {
                    state: Self::compact(self),
                    src,
                    end: required.get(),
                });
                return Ok(());
            }
            let cap = self.state.cap;
            return self.fresh(cap, required, src, required.get());
        };
        let ideal = window::Growth::new(self.state.cap, ideal).unwrap_or(growth);
        self.try_grow(ideal, growth, required.get(), src)
    }

    fn try_grow<I: buffer::raw::SliceExt>(
        self,
        ideal: window::Growth<MAX_CAPACITY>,
        required: window::Growth<MAX_CAPACITY>,
        end: u32,
        src: I,
    ) -> Result<(), alloc::Error<P::Failure>> {
        let window::State { buf, cap, head } = self.state;
        match buf.access() {
            alloc::Access::Unique(source) => {
                let target =
                    if ideal.amount().get() as usize <= P::available(source.policy().seed()) {
                        ideal
                    } else {
                        required
                    };
                let growth = Resize {
                    source,
                    ideal: target,
                    need: required,
                    range: *self.tail..*head,
                    tail: src,
                }
                .try_commit()?;
                *cap = growth.capacity();
                *head = end;
                *self.tail = 0;
                Ok(())
            }
            alloc::Access::Shared => self.fresh(ideal.capacity(), required.capacity(), src, end),
        }
    }

    fn fresh<I: buffer::raw::SliceExt>(
        self,
        ideal: window::Capacity<MAX_CAPACITY>,
        required: window::Capacity<MAX_CAPACITY>,
        src: I,
        len: u32,
    ) -> Result<(), alloc::Error<P::Failure>> {
        let state = self.state;
        // SAFETY: Edit retains the source owner's backing and its exact suffix cursor.
        let first = unsafe {
            buffer::raw::range(
                state.buf.data(),
                *self.tail as usize,
                (state.head - *self.tail) as usize,
            )
        };
        let next = Fresh {
            ideal,
            need: required,
            first,
            second: src,
            len,
        }
        .try_allocate(state.seed())?;
        *state = next;
        *self.tail = 0;
        Ok(())
    }
}

impl<'a, const MAX: usize> Edit<'a, (), MAX> {
    pub(in crate::buffer::bytes::window) fn new(
        state: &'a mut window::State<(), MAX>,
        tail: &'a mut u32,
    ) -> Self {
        Self { state, tail }
    }

    pub(in crate::buffer::bytes::window) fn try_as_mut(
        self,
    ) -> Result<&'a mut [u8], storage::AllocError> {
        if self.state.head == *self.tail {
            return Ok(&mut []);
        }
        if !self.state.buf.is_unique() {
            let cap = self.state.cap;
            let len = self.state.head - *self.tail;
            Edit::new(&mut *self.state, &mut *self.tail)
                .fresh(cap, cap, &[][..], len)
                .map_err(alloc::Error::into_alloc)
                .map_err(storage::AllocError::Exhausted)?;
        }
        let len = (self.state.head - *self.tail) as usize;
        // SAFETY: uniqueness admission or fresh replacement retains this exact
        // initialized suffix exclusively for the original mutable owner loan.
        Ok(unsafe {
            buffer::raw::range_mut(self.state.buf.data().cast_mut(), *self.tail as usize, len)
        })
    }

    pub(in crate::buffer::bytes::window) fn try_slices<const N: usize>(
        self,
        source: owners::Measured<[&[u8]; N]>,
    ) -> Result<(), storage::AllocError> {
        let Some(added) = num::NonZero::new(source.len()) else {
            return Ok(());
        };
        if ((self.state.cap.get() - self.state.head) as usize) < added.get() {
            let len = self.state.head - *self.tail;
            let required = added.saturating_add(len as usize);
            let required =
                window::Capacity::try_new(required).map_err(storage::AllocError::Capacity)?;
            let ideal = self.state.cap.grow(required);
            return self
                .try_full(source.into_input(), required, ideal)
                .map_err(alloc::Error::into_alloc)
                .map_err(storage::AllocError::Exhausted);
        }
        append_slices(self.state, source.into_input().into_iter());
        Ok(())
    }

    pub(in crate::buffer::bytes::window) fn try_extend(
        self,
        src: &'a [u8],
    ) -> Result<(), storage::AllocError> {
        if (self.state.cap.get() - self.state.head) as usize >= src.len() {
            buffer::raw::Copy::copy(Append {
                end: self.state.head + src.len() as u32,
                state: self.state,
                src,
            });
            return Ok(());
        }
        let required =
            window::Capacity::try_extend(&self, src).map_err(storage::AllocError::Capacity)?;
        let ideal = self.state.cap.grow(required);
        self.try_full(src, required, ideal)
            .map_err(alloc::Error::into_alloc)
            .map_err(storage::AllocError::Exhausted)
    }
}

impl<'a, 'd> Edit<'a, resident::Lease<'d>, { u32::MAX as usize }> {
    /// Consumes strict growth bound to this unchanged exclusive state.
    pub(in crate::buffer) fn try_reserve(
        reserve: staging::Reserve<
            (
                &'a mut window::State<resident::Lease<'d>, { u32::MAX as usize }>,
                &'a mut u32,
            ),
            (),
        >,
    ) -> Result<(), alloc::Error<credit::Error>> {
        let ((state, tail), target, ()) = reserve.into_parts();
        let growth = window::Growth {
            capacity: target.into(),
            // SAFETY: the sealed reserve retains strict growth for this unchanged state.
            amount: unsafe { crate::mem::raw::Counter::nonzero(target.get() - state.cap.get()) },
        };
        let len = state.head - *tail;
        Self { state, tail }.try_grow(growth, growth, len, &[][..])
    }

    pub(in crate::buffer) fn try_staged<I: buffer::raw::SliceExt>(
        append: staging::Reserve<
            (
                &'a mut window::State<resident::Lease<'d>, { u32::MAX as usize }>,
                &'a mut u32,
            ),
            owners::Measured<I>,
        >,
    ) -> Result<(), alloc::Error<credit::Error>> {
        let ((state, tail), target, src) = append.into_parts();
        let added = src.len();
        let src = src.into_input();
        let ideal = window::Capacity::from(target);
        if (state.cap.get() - state.head) as usize >= added {
            let end = state.head + added as u32;
            buffer::raw::Copy::copy(Append { state, src, end });
            return Ok(());
        }
        let required = num::NonZero::<u32>::MIN
            .saturating_add(((state.head - *tail) as usize + added - 1) as u32);
        Self { state, tail }.try_full(src, required.into(), ideal)
    }
}

impl<P: buffer::raw::Policy, const MAX_CAPACITY: usize> Edit<'_, P, MAX_CAPACITY> {
    fn compact(edit: Edit<'_, P, MAX_CAPACITY>) -> &mut window::State<P, MAX_CAPACITY> {
        let state = edit.state;
        let tail = *edit.tail as usize;
        let head = state.head as usize;
        let len = head - tail;
        if tail != 0 && len != 0 {
            // SAFETY: the owner-bound plan proved uniqueness and both ranges
            // fit the initialized prefix; compaction may overlap its source.
            let pointer = state.buf.data().cast_mut();
            unsafe { ptr::copy(pointer.add(tail), pointer, len) };
        }
        state.head = len as u32;
        *edit.tail = 0;
        state
    }
}

fn append_slices<'a, const MAX: usize>(
    state: &mut window::State<(), MAX>,
    sources: impl Iterator<Item = &'a [u8]>,
) {
    for src in sources {
        if !src.is_empty() {
            buffer::raw::Copy::copy(Append {
                end: state.head + src.len() as u32,
                state: &mut *state,
                src,
            });
        }
    }
}

struct Append<'a, P: crate::buffer::raw::Policy, const MAX: usize, I> {
    state: &'a mut window::State<P, MAX>,
    src: I,
    end: u32,
}

// SAFETY: Edit admitted this source against the exact unchanged exclusive suffix.
unsafe impl<P: crate::buffer::raw::Policy, const MAX: usize, I: buffer::raw::SliceExt>
    buffer::raw::Copy for Append<'_, P, MAX, I>
{
    type Output = ();

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (
            &self.src,
            self.state.buf.data().cast_mut(),
            self.state.head as usize,
        )
    }

    unsafe fn publish(self) {
        self.state.head = self.end;
    }
}

struct Init<P: crate::buffer::raw::Policy, const MAX: usize, I> {
    heap: crate::buffer::raw::Heap<P>,
    cap: window::Capacity<MAX>,
    sources: I,
    len: u32,
}

// SAFETY: Fresh retains the checked total and allocation covering both sources.
unsafe impl<P: crate::buffer::raw::Policy, const MAX: usize, I: buffer::raw::SliceExt>
    buffer::raw::Copy for Init<P, MAX, I>
{
    type Output = window::State<P, MAX>;

    fn parts(&mut self) -> (impl buffer::raw::SliceExt, *mut u8, usize) {
        (&self.sources, self.heap.as_mut_ptr().cast(), 0)
    }

    unsafe fn publish(self) -> Self::Output {
        window::State {
            buf: self.heap.into_shared(),
            cap: self.cap,
            head: self.len,
        }
    }
}

pub(in crate::buffer) struct Fresh<'a, const MAX: usize, I = &'a [u8]> {
    ideal: window::Capacity<MAX>,
    need: window::Capacity<MAX>,
    first: &'a [u8],
    second: I,
    len: u32,
}

impl<const MAX: usize> Fresh<'_, MAX> {
    pub(in crate::buffer::bytes::window) fn try_slices<const N: usize>(
        source: owners::Measured<[&[u8]; N]>,
    ) -> Result<Option<window::State<(), MAX>>, storage::AllocError> {
        let len = source.len();
        let mut sources = source.into_input().into_iter();
        let Some(first) = sources.find(|source| !source.is_empty()) else {
            return Ok(None);
        };
        let capacity = num::NonZero::<usize>::MIN.saturating_add(len - 1);
        let mut state = Self::try_copy(first, capacity)?;
        append_slices(&mut state, sources);
        Ok(Some(state))
    }

    pub(in crate::buffer::bytes::window) fn try_copy(
        src: &[u8],
        capacity: num::NonZero<usize>,
    ) -> Result<window::State<(), MAX>, storage::AllocError> {
        let initial = window::Capacity::try_new(capacity).map_err(storage::AllocError::Capacity)?;
        let need = if src.len() > initial.get() as usize {
            let capacity = num::NonZero::<usize>::MIN.saturating_add(src.len() - 1);
            window::Capacity::try_new(capacity).map_err(storage::AllocError::Capacity)?
        } else {
            initial
        };
        Fresh {
            ideal: initial.grow(need),
            need,
            first: src,
            second: [] as [&[u8]; 0],
            len: src.len() as u32,
        }
        .try_allocate(())
        .map_err(alloc::Error::into_alloc)
        .map_err(storage::AllocError::Exhausted)
    }
}

impl<const MAX: usize, I: buffer::raw::SliceExt> Fresh<'_, MAX, I> {
    fn try_allocate<P: buffer::raw::Policy>(
        self,
        seed: P::Seed,
    ) -> Result<window::State<P, MAX>, alloc::Error<P::Failure>> {
        let Self {
            ideal,
            need,
            first,
            second,
            len,
        } = self;
        let target = if ideal.get() as usize <= P::available(seed) {
            ideal
        } else {
            need
        };
        let (heap, cap) = match crate::buffer::raw::Heap::try_new(seed, target.nonzero()) {
            Ok(heap) => (heap, target),
            Err(alloc::Error::Allocation(crate::alloc::Exhausted)) if target != need => (
                crate::buffer::raw::Heap::try_new(seed, need.nonzero())?,
                need,
            ),
            Err(error) => return Err(error),
        };
        Ok(buffer::raw::Copy::copy(Init {
            heap,
            cap,
            sources: (first, second),
            len,
        }))
    }
}

const _: () = {
    assert!(size_of::<Fresh<'static, 1>>() == 6 * size_of::<usize>());
    assert!(!mem::needs_drop::<Fresh<'static, 1>>());
    assert!(size_of::<Resize<'static, (), 1>>() == 6 * size_of::<usize>());
    assert!(size_of::<Option<Resize<'static, (), 1>>>() == 6 * size_of::<usize>());
};

impl Fresh<'_, { u32::MAX as usize }> {
    pub(in crate::buffer) fn try_staged<'d, I: buffer::raw::SliceExt>(
        reserve: staging::Reserve<resident::Budget<'d>, (&[u8], owners::Measured<I>)>,
    ) -> Result<
        window::State<resident::Lease<'d>, { u32::MAX as usize }>,
        alloc::Error<credit::Error>,
    > {
        let (budget, target, (first, second)) = reserve.into_parts();
        let capacity = window::Capacity::from(target);
        let len = (first.len() + second.len()) as u32;
        let fresh = Fresh {
            ideal: capacity,
            need: capacity,
            first,
            second: second.into_input(),
            len,
        };
        fresh.try_allocate(budget)
    }
}

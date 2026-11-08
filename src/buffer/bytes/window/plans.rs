use std::{num, ptr};

use storage::alloc;

use crate::{
    buffer,
    buffer::{bytes::window, pool::owners, resident, resident::staging, storage},
    mem::credit,
};

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
                append(Self::compact(self), src, required.get());
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
            Some(mut source) => {
                let mut growth = if ideal.amount().get() as usize <= source.policy().available() {
                    ideal
                } else {
                    required
                };
                let range = *self.tail..*head;
                let at_start = range.start == 0;
                let growth = loop {
                    let target = growth.capacity().nonzero();
                    let error = {
                        let debit =
                            crate::buffer::raw::Debit::try_new(*source.policy(), growth.amount())
                                .map_err(alloc::Error::Budget)?;
                        if at_start {
                            match source.try_resize(target, debit) {
                                Ok(()) => {
                                    // SAFETY: Edit admitted this disjoint tail after the
                                    // initialized prefix; resizing preserves that prefix.
                                    unsafe {
                                        src.copy_to(
                                            source.data().cast_mut().add(range.end as usize),
                                        );
                                    }
                                    break growth;
                                }
                                Err(error) => error,
                            }
                        } else {
                            let plan = crate::buffer::raw::Header::<P>::layout(target);
                            // SAFETY: this admitted header includes a nonzero target capacity.
                            match unsafe { crate::alloc::raw::try_new(plan, false) } {
                                Ok(pointer) => {
                                    source.replace(pointer, target, debit, range, src);
                                    break growth;
                                }
                                Err(error) => error,
                            }
                        }
                    };
                    if growth.capacity() == required.capacity() {
                        return Err(alloc::Error::Allocation(error));
                    }
                    growth = required;
                };
                *cap = growth.capacity();
                *head = end;
                *self.tail = 0;
                Ok(())
            }
            None => self.fresh(ideal.capacity(), required.capacity(), src, end),
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
        let next = try_allocate(state.seed(), ideal, required, (first, src), len)?;
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
            let end = self.state.head + src.len() as u32;
            append(self.state, src, end);
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

impl<'a, 'd> Edit<'a, resident::Budget<'d>, { u32::MAX as usize }> {
    /// Consumes strict growth bound to this unchanged exclusive state.
    pub(in crate::buffer) fn try_reserve(
        reserve: staging::Reserve<
            (
                &'a mut window::State<resident::Budget<'d>, { u32::MAX as usize }>,
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
        reserve: staging::Reserve<
            (
                &'a mut window::State<resident::Budget<'d>, { u32::MAX as usize }>,
                &'a mut u32,
            ),
            owners::Measured<I>,
        >,
    ) -> Result<(), alloc::Error<credit::Error>> {
        let ((state, tail), target, src) = reserve.into_parts();
        let added = src.len();
        let src = src.into_input();
        let ideal = window::Capacity::from(target);
        if (state.cap.get() - state.head) as usize >= added {
            let end = state.head + added as u32;
            append(state, src, end);
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
            let end = state.head + src.len() as u32;
            append(state, src, end);
        }
    }
}

fn append<P: buffer::raw::Policy, const MAX: usize, I: buffer::raw::SliceExt>(
    state: &mut window::State<P, MAX>,
    src: I,
    end: u32,
) {
    let target = state.buf.data().cast_mut();
    // SAFETY: Edit admitted this source against the unchanged exclusive suffix.
    unsafe { src.copy_to(target.add(state.head as usize)) };
    state.head = end;
}

pub(in crate::buffer::bytes::window) fn try_slices<const MAX: usize, const N: usize>(
    source: owners::Measured<[&[u8]; N]>,
) -> Result<Option<window::State<(), MAX>>, storage::AllocError> {
    let len = source.len();
    let mut sources = source.into_input().into_iter();
    let Some(first) = sources.find(|source| !source.is_empty()) else {
        return Ok(None);
    };
    let capacity = num::NonZero::<usize>::MIN.saturating_add(len - 1);
    let mut state = try_copy(first, capacity)?;
    append_slices(&mut state, sources);
    Ok(Some(state))
}

#[inline]
pub(in crate::buffer::bytes::window) fn try_copy<const MAX: usize>(
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
    try_allocate((), initial.grow(need), need, src, src.len() as u32)
        .map_err(alloc::Error::into_alloc)
        .map_err(storage::AllocError::Exhausted)
}

fn try_allocate<P: buffer::raw::Policy, const MAX: usize, I: buffer::raw::SliceExt>(
    seed: P,
    ideal: window::Capacity<MAX>,
    need: window::Capacity<MAX>,
    sources: I,
    len: u32,
) -> Result<window::State<P, MAX>, alloc::Error<P::Failure>> {
    let target = if ideal.get() as usize <= P::available(seed) {
        ideal
    } else {
        need
    };
    let (mut heap, cap) = match crate::buffer::raw::Heap::try_new(seed, target.nonzero()) {
        Ok(heap) => (heap, target),
        Err(alloc::Error::Allocation(crate::alloc::Exhausted)) if target != need => (
            crate::buffer::raw::Heap::try_new(seed, need.nonzero())?,
            need,
        ),
        Err(error) => return Err(error),
    };
    // SAFETY: the admitted sources fit this fresh allocation, which remains
    // exclusively owned until the complete initialized prefix is published.
    unsafe { sources.copy_to(heap.as_mut_ptr().cast()) };
    Ok(window::State {
        buf: heap.into_shared(),
        cap,
        head: len,
    })
}

pub(in crate::buffer) fn try_staged<'d, I: buffer::raw::SliceExt>(
    reserve: staging::Reserve<resident::Budget<'d>, (&[u8], owners::Measured<I>)>,
) -> Result<window::State<resident::Budget<'d>, { u32::MAX as usize }>, alloc::Error<credit::Error>>
{
    let (budget, target, (first, second)) = reserve.into_parts();
    let capacity = window::Capacity::from(target);
    let len = (first.len() + second.len()) as u32;
    try_allocate(
        budget,
        capacity,
        capacity,
        (first, second.into_input()),
        len,
    )
}

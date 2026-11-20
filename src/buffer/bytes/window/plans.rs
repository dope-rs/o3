use std::{num, ptr};

use storage::alloc;

use crate::{
    buffer,
    buffer::{bytes::window, pool::owners, resident, resident::staging, storage},
    mem::credit,
};

impl<P: buffer::raw::Policy, const MAX_CAPACITY: usize> window::State<P, MAX_CAPACITY> {
    fn append_end(&self, added: usize) -> Option<u32> {
        if added <= (self.cap.get() - self.head) as usize {
            Some(self.head + added as u32)
        } else {
            None
        }
    }

    fn append_at<I: buffer::raw::SliceExt>(&mut self, start: u32, src: I, end: u32) {
        let target = self.buf.data().cast_mut();
        // SAFETY: the owner admitted this disjoint source for the suffix at start.
        unsafe { src.copy_to(target.add(start as usize)) };
        self.head = end;
    }

    fn try_full<I: buffer::raw::SliceExt>(
        &mut self,
        tail: &mut u32,
        src: I,
        required: window::Capacity<MAX_CAPACITY>,
        ideal: window::Capacity<MAX_CAPACITY>,
    ) -> Result<(), alloc::Error<P::Failure>> {
        let Some(growth) = window::Growth::new(self.cap, required) else {
            if !self.buf.is_unique() {
                return self.fresh(tail, self.cap, required, src, required.get());
            }
            let start = *tail;
            let len = self.head - start;
            if start != 0 && len != 0 {
                let pointer = self.buf.data().cast_mut();
                // SAFETY: this is the sole owner of the initialized suffix.
                unsafe { ptr::copy(pointer.add(start as usize), pointer, len as usize) };
            }
            self.append_at(len, src, required.get());
            *tail = 0;
            return Ok(());
        };
        let ideal = window::Growth::new(self.cap, ideal).unwrap_or(growth);
        self.try_grow(tail, ideal, growth, required.get(), src)
    }

    fn try_grow<I: buffer::raw::SliceExt>(
        &mut self,
        tail: &mut u32,
        ideal: window::Growth<MAX_CAPACITY>,
        required: window::Growth<MAX_CAPACITY>,
        end: u32,
        src: I,
    ) -> Result<(), alloc::Error<P::Failure>> {
        let Self { buf, cap, head } = self;
        match buf.access() {
            Some(mut source) => {
                let mut growth = if ideal.amount().get() as usize <= source.policy().available() {
                    ideal
                } else {
                    required
                };
                let start = *tail;
                let range = start..*head;
                let growth = loop {
                    let target = growth.capacity().nonzero();
                    let error = {
                        let debit =
                            crate::buffer::raw::Debit::try_new(*source.policy(), growth.amount())
                                .map_err(alloc::Error::Budget)?;
                        if start == 0 {
                            match source.try_resize(target, debit) {
                                Ok(()) => {
                                    // SAFETY: resizing preserves the initialized prefix and
                                    // the admitted target has room for this disjoint source.
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
                *tail = 0;
                Ok(())
            }
            None => self.fresh(tail, ideal.capacity(), required.capacity(), src, end),
        }
    }

    fn fresh<I: buffer::raw::SliceExt>(
        &mut self,
        tail: &mut u32,
        ideal: window::Capacity<MAX_CAPACITY>,
        required: window::Capacity<MAX_CAPACITY>,
        src: I,
        len: u32,
    ) -> Result<(), alloc::Error<P::Failure>> {
        // SAFETY: the owner's cursor lies within this unchanged initialized prefix.
        let first = unsafe { self.as_slice().get_unchecked(*tail as usize..) };
        *self = try_allocate(self.seed(), ideal, required, (first, src), len)?;
        *tail = 0;
        Ok(())
    }
}

impl<const MAX: usize> window::State<(), MAX> {
    pub(in crate::buffer::bytes::window) fn try_as_mut<'a>(
        &'a mut self,
        tail: &'a mut u32,
    ) -> Result<&'a mut [u8], storage::AllocError> {
        let len = self.head - *tail;
        if len == 0 {
            return Ok(&mut []);
        }
        if !self.buf.is_unique() {
            self.fresh(tail, self.cap, self.cap, &[][..], len)
                .map_err(alloc::Error::into_alloc)
                .map_err(storage::AllocError::Exhausted)?;
        }
        // SAFETY: uniqueness admission or fresh replacement retains this exact
        // initialized suffix exclusively for the original mutable owner loan.
        Ok(unsafe {
            buffer::raw::range_mut(self.buf.data().cast_mut(), *tail as usize, len as usize)
        })
    }
}

impl<const MAX: usize> buffer::write::Sink for window::Snapshot<MAX> {
    type Error = storage::AllocError;

    fn write_slice(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if bytes.is_empty() {
            return Ok(());
        }
        if let Some(state) = &mut self.state
            && let Some(end) = state.append_end(bytes.len())
        {
            state.append_at(state.head, bytes, end);
            return Ok(());
        }
        let required = window::Capacity::try_append(self, bytes.len(), &[])
            .map_err(storage::AllocError::Capacity)?;
        self.try_store::<false, _>(bytes, required)
    }

    fn write_slices<const N: usize>(&mut self, slices: [&[u8]; N]) -> Result<(), Self::Error> {
        if let [bytes] = slices.as_slice() {
            return self.write_slice(bytes);
        }
        let required = window::Capacity::try_append(self, 0, &slices)
            .map_err(storage::AllocError::Capacity)?;
        self.try_store::<true, _>(&slices, required)
    }
}

impl<const MAX: usize> window::Snapshot<MAX> {
    fn try_store<const CHECK_SPARE: bool, I: buffer::raw::SliceExt>(
        &mut self,
        src: I,
        required: Option<window::Capacity<MAX>>,
    ) -> Result<(), storage::AllocError> {
        let Some(required) = required else {
            return Ok(());
        };
        match &mut self.state {
            Some(state) => {
                if CHECK_SPARE && required.get() <= state.cap.get() - self.tail {
                    state.append_at(state.head, src, self.tail + required.get());
                    return Ok(());
                }
                state.try_full(&mut self.tail, src, required, state.cap.grow(required))
            }
            slot @ None => try_allocate((), required, required, src, required.get())
                .map(|state| *slot = Some(state)),
        }
        .map_err(alloc::Error::into_alloc)
        .map_err(storage::AllocError::Exhausted)
    }
}

impl<'d> window::State<resident::Budget<'d>, { u32::MAX as usize }> {
    /// Consumes strict growth bound to this unchanged exclusive state.
    pub(in crate::buffer) fn try_reserve(
        reserve: staging::Reserve<(&mut Self, &mut u32), ()>,
    ) -> Result<(), alloc::Error<credit::Error>> {
        let ((state, tail), target, ()) = reserve.into_parts();
        let growth = window::Growth {
            capacity: target.into(),
            // SAFETY: the sealed reserve retains strict growth for this unchanged state.
            amount: unsafe { crate::mem::raw::Counter::nonzero(target.get() - state.cap.get()) },
        };
        let len = state.head - *tail;
        state.try_grow(tail, growth, growth, len, &[][..])
    }

    pub(in crate::buffer) fn try_staged<I: buffer::raw::SliceExt>(
        reserve: staging::Reserve<(&mut Self, &mut u32), owners::Measured<I>>,
    ) -> Result<(), alloc::Error<credit::Error>> {
        let ((state, tail), target, src) = reserve.into_parts();
        if let Some(end) = state.append_end(src.len()) {
            state.append_at(state.head, src.into_input(), end);
            return Ok(());
        }
        let required = num::NonZero::<u32>::MIN
            .saturating_add(((state.head - *tail) as usize + src.len() - 1) as u32);
        state.try_full(tail, src.into_input(), required.into(), target.into())
    }
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

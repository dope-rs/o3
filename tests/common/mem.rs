use std::{cell::Cell, marker, num::NonZero, ptr};

use o3::{
    alloc,
    mem::{self, Return, credit::fair},
};

pub struct Pinned<'a> {
    pub address: Cell<*const Self>,
    pub drops: super::Counted<&'a Cell<Option<usize>>>,
    pub pin: marker::PhantomPinned,
}

impl Drop for Pinned<'_> {
    fn drop(&mut self) {
        assert_eq!(self.address.get(), ptr::from_ref(self));
    }
}

pub struct Lanes<const N: usize = 1, const L: usize = 1> {
    pool: mem::raw::Pool<N, (), u32>,
    states: [mem::raw::State<N, (), u32>; L],
}

impl<const N: usize, const L: usize> Lanes<N, L> {
    pub fn try_new(shared: [u32; N], reserves: [[u32; N]; L]) -> Result<Self, fair::Cause<u32, N>> {
        for reserve in reserves {
            for axis in fair::Axis::<N>::ALL {
                let index = axis.index();
                shared[index]
                    .checked_add(reserve[index])
                    .ok_or(fair::Cause::Overflow {
                        axis,
                        held: shared[index],
                        requested: reserve[index],
                    })?;
            }
        }
        Ok(Self {
            pool: mem::raw::Pool::new(shared),
            states: reserves.map(mem::raw::State::new),
        })
    }

    pub fn shared(&self) -> [u32; N] {
        self.pool.shared()
    }

    pub fn get(&self, index: usize) -> Option<mem::raw::Lane<'_, '_, N, (), u32>> {
        let state = self.states.get(index)?;
        // SAFETY: this owner retains the checked pool and unique inline
        // states; every returned lane and grant borrows this owner.
        Some(unsafe { self.pool.lane(state) })
    }
}

#[derive(Clone, Copy)]
pub struct Reentry<'p, 's> {
    pub lane: mem::raw::Lane<'p, 'p, 1, (), u32>,
    pub other: mem::raw::Lane<'p, 'p, 1, (), u32>,
    pub acquire: &'s Cell<Option<u32>>,
    pub nested: &'s Cell<Option<mem::raw::Charge<mem::raw::Lane<'p, 'p, 1, (), u32>>>>,
    pub failure: &'s Cell<Option<fair::Cause<u32, 1>>>,
}

// SAFETY: every copy retains the same lane. Reentry uses ordinary acquisitions
// on the separately retained lane, preserving every outstanding charge.
unsafe impl mem::raw::Source<1> for Reentry<'_, '_> {
    type Tag = ();
    type Word = u32;

    unsafe fn accounts(&self) -> mem::raw::Accounts<'_, 1, (), u32> {
        if let Some(amount) = self.acquire.take() {
            match self.other.try_acquire([amount]) {
                Ok(charge) => drop(self.nested.replace(Some(charge))),
                Err(error) => self.failure.set(Some(error.into_parts().1)),
            }
        }
        // SAFETY: the original lane keeps its immutable live pool/state binding.
        unsafe { mem::raw::Source::accounts(&self.lane) }
    }
}

pub enum Root {}
pub enum Child {}

#[repr(transparent)]
pub struct Edge<W: mem::raw::Counter = u32>(pub mem::raw::Count<W>);

// SAFETY: each test value owns one complete, exclusively driven edge domain.
unsafe impl<W: mem::raw::Counter> mem::raw::Edge<W> for Edge<W> {
    fn count(&self) -> &mem::raw::Count<W> {
        &self.0
    }
}

pub struct Sink<'a> {
    pub returned: &'a Cell<Option<u8>>,
    pub calls: &'a Cell<Option<u8>>,
}

impl Return for Sink<'_> {
    type Item = u8;

    fn return_item(self, item: u8) {
        self.calls
            .set(self.calls.get().and_then(|count| count.checked_add(1)));
        self.returned.set(Some(item));
    }
}

pub struct RefSink<'a>(pub &'a Cell<Option<usize>>);

impl Return for RefSink<'_> {
    type Item = ();

    fn return_item(self, (): ()) {
        self.0
            .set(self.0.get().and_then(|count| count.checked_add(1)));
    }
}

pub struct Target<'a, W: mem::raw::Counter = u32> {
    pub refs: mem::raw::Count<W>,
    pub value: usize,
    pub finished: &'a Cell<Option<usize>>,
}

impl<'a, W: mem::raw::Counter> Target<'a, W> {
    pub fn try_owner(
        finished: &'a Cell<Option<usize>>,
    ) -> Result<mem::raw::Owner<Self, W>, alloc::Exhausted> {
        let target = alloc::BoxExt::try_new(Self {
            refs: mem::raw::Count::one(),
            value: 17,
            finished,
        })?;
        // SAFETY: the allocation transfers its one initialized counted edge.
        Ok(unsafe { mem::raw::Owner::from_raw(ptr::NonNull::from(Box::leak(target))) })
    }
}

// SAFETY: the initialized target retains its own immutable reference domain.
unsafe impl<W: mem::raw::Counter> mem::raw::Edge<W> for Target<'_, W> {
    fn count(&self) -> &mem::raw::Count<W> {
        &self.refs
    }
}

// SAFETY: every admitted owner uses this exact boxed target until its last edge.
unsafe impl<W: mem::raw::Counter> mem::raw::Target<W> for Target<'_, W> {
    unsafe fn destroy(pointer: ptr::NonNull<Self>) {
        // SAFETY: the terminal edge transfers the complete boxed allocation.
        let target = unsafe { Box::from_raw(pointer.as_ptr()) };
        target
            .finished
            .set(target.finished.get().and_then(|count| count.checked_add(1)));
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Held {
    pub resident: u32,
    pub bytes: NonZero<u32>,
}

pub struct Partition(pub [u32; 2]);
pub struct Attach(pub NonZero<u32>);
pub struct Bytes(pub NonZero<u32>);

// SAFETY: every value represents exactly one entry and its resident component.
unsafe impl mem::raw::Amount<2, u32> for Held {
    fn get(self) -> [u32; 2] {
        [1, self.resident]
    }
}

// SAFETY: binding admits a remainder exactly one entry below the source;
// byte metadata cannot change either component of that partition.
unsafe impl mem::raw::Exact<2, u32, [u32; 2]> for Partition {
    type Input = NonZero<u32>;
    type Left = [u32; 2];
    type Right = Held;

    fn parts(self, source: [u32; 2], bytes: Self::Input) -> (Self::Left, Held) {
        (
            self.0,
            Held {
                resident: source[1] - self.0[1],
                bytes,
            },
        )
    }
}

// SAFETY: binding restricts this transformation to a one-entry source.
unsafe impl mem::raw::ExactRebind<2, u32, [u32; 2]> for Attach {
    type Output = Held;

    fn rebind(self, source: [u32; 2]) -> Held {
        Held {
            resident: source[1],
            bytes: self.0,
        }
    }
}

// SAFETY: the only changed field is representation-only byte metadata.
unsafe impl mem::raw::ExactRebind<2, u32, Held> for Bytes {
    type Output = Held;

    fn rebind(self, source: Held) -> Held {
        Held {
            bytes: self.0,
            ..source
        }
    }
}

// SAFETY: byte metadata is independent of both components for every Held.
unsafe impl mem::raw::Rebind<2, u32, Held> for Bytes {}

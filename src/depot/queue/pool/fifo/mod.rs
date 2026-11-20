use std::mem;

use crate::depot::arena::{
    linked,
    linked::{fifo::ends, slots},
};

/// An independently movable FIFO borrowing its exact fixed node pool.
pub struct Fifo<'pool, T, const TRACK: bool = true> {
    pub(in crate::depot) arena: &'pool crate::depot::raw::Arena<T, (), TRACK>,
    pub(in crate::depot) links: ends::Exclusive<slots::Index>,
}

impl<'pool, T, const TRACK: bool> Fifo<'pool, T, TRACK> {
    pub(in crate::depot) const fn new(
        arena: &'pool crate::depot::raw::Arena<T, (), TRACK>,
    ) -> Self {
        Self {
            arena,
            links: ends::Exclusive::empty(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.links.is_empty()
    }

    pub fn front(&mut self) -> Option<Front<'_, 'pool, T, TRACK>> {
        let index = self.links.head()?;
        Some(Front { fifo: self, index })
    }

    pub fn pop_front(&mut self) -> Option<T> {
        Some(self.front()?.pop())
    }

    pub fn iter(&self) -> linked::Iter<'_, T> {
        // SAFETY: this FIFO borrow retains its exact arena partition unchanged.
        unsafe { linked::Iter::new(&self.arena.inner, self.links.head_raw()) }
    }
}

/// A nonempty front retaining the exact exclusive FIFO borrow.
pub struct Front<'fifo, 'pool, T, const TRACK: bool = true> {
    fifo: &'fifo mut Fifo<'pool, T, TRACK>,
    index: slots::Index,
}

impl<'pool, T, const TRACK: bool> Front<'_, 'pool, T, TRACK> {
    pub fn get(&self) -> &T {
        // SAFETY: this exclusive FIFO borrow owns the initialized front.
        unsafe { self.fifo.arena.inner.value(self.index) }
    }

    pub fn pop(self) -> T {
        unsafe {
            self.fifo
                .links
                .pop::<_, TRACK>(&self.fifo.arena.inner, self.index)
        }
    }

    pub fn detach(self) -> slots::Live<T, slots::Borrow<'pool, T, TRACK>> {
        unsafe {
            self.fifo
                .links
                .detach(slots::Borrow(&self.fifo.arena.inner), self.index)
        }
    }
}

const _: () =
    assert!(mem::size_of::<Front<'static, 'static, u64>>() == 2 * mem::size_of::<usize>());

impl<'pool, T> Fifo<'pool, T> {
    pub fn vacant(&mut self) -> Option<slots::Vacant<T, &mut Fifo<'pool, T>>> {
        // SAFETY: this FIFO uses its store's tracked admission and return.
        unsafe { slots::Vacant::try_reserve(self) }
    }
}

// SAFETY: the borrowed FIFO retains its original allocation until release.
impl<T, const TRACK: bool> linked::Sealed<T> for &mut Fifo<'_, T, TRACK> {}

impl<T, const TRACK: bool> linked::Source<T> for &mut Fifo<'_, T, TRACK> {
    const TRACK: bool = TRACK;

    fn store(&self) -> &slots::Store<T> {
        &self.arena.inner
    }
}

impl<T, const TRACK: bool> Drop for Fifo<'_, T, TRACK> {
    fn drop(&mut self) {
        while self.pop_front().is_some() {}
    }
}

const _: () = assert!(mem::size_of::<Fifo<'static, u64>>() == 2 * mem::size_of::<usize>());

impl<'pool, T, const TRACK: bool> Fifo<'pool, T, TRACK> {
    /// Reserves with one source that retains its exact unary credit through completion.
    pub fn reserve<'fifo, S>(source: S) -> S::Reserved
    where
        'pool: 'fifo,
        S: crate::depot::raw::Reserve<'fifo, 'pool, TRACK, Item = T>,
    {
        // SAFETY: this admitted source retains both original reservation owners.
        let (fifo, credit) = unsafe { source.into_parts() };
        // SAFETY: the paired credit reserves one unused slot in this exact arena.
        let slot = unsafe { slots::Vacant::reserve(fifo) };
        // SAFETY: the native output retains these same paired owners, including cancellation order.
        unsafe { S::build(slot, credit) }
    }
}

impl<'fifo, 'pool, T, const TRACK: bool> slots::Vacant<T, &'fifo mut Fifo<'pool, T, TRACK>> {
    /// Initializes and publishes this reserved node into its original FIFO.
    pub fn push_back(self, value: T) {
        let mut live = self.insert(value);
        let index = live.index();
        // SAFETY: publication changes only the retained FIFO's queue state.
        let fifo = unsafe { live.owner_mut() };
        unsafe { fifo.links.append_end(&fifo.arena.inner, index) };
        unsafe { live.publish() };
    }
}

const _: () = assert!(
    mem::size_of::<slots::Vacant<u64, &'static mut Fifo<'static, u64>>>()
        == 2 * mem::size_of::<usize>()
);

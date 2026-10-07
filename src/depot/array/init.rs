//! In-place initialization whose guard borrows the backing array.

use std::{iter, marker, mem, ops, ptr, slice};

use crate::depot::{array, array::prefix, capacity};

/// A borrowed prefix whose initialized positions retain one invariant identity.
pub struct Prefix<'a, 'id, T, const N: usize> {
    entries: &'a mut [mem::MaybeUninit<T>; N],
    len: array::Len<N>,
    brand: marker::PhantomData<*mut capacity::Domain<'id>>,
}

/// Exclusive admission to initialize one exact position in a scoped prefix.
#[must_use = "a prefix slot must be filled or abandoned"]
pub struct Slot<'a, 'b, 'id, T, const N: usize> {
    prefix: &'a mut Prefix<'b, 'id, T, N>,
    slot: array::Vacancy<N>,
}

impl<'a, T, const N: usize> Prefix<'a, '_, T, N> {
    /// Borrows reusable storage and releases every initialized value on return.
    /// The operation can borrow the guard but cannot take its destruction duty.
    pub fn with<U, R>(
        entries: &'a mut [mem::MaybeUninit<U>; N],
        run: impl for<'id> FnOnce(&mut Prefix<'a, 'id, T, N>) -> R,
    ) -> R {
        const {
            assert!(N <= u8::MAX as usize + 1);
            assert!(size_of::<T>() == size_of::<U>(), "size");
            assert!(align_of::<T>() == align_of::<U>(), "align");
        };
        let mut prefix = Prefix {
            entries: crate::mem::raw::cast_uninit(entries),
            len: array::Len::ZERO,
            brand: marker::PhantomData,
        };
        run(&mut prefix)
    }
}

impl<'a, 'id, T, const N: usize> Prefix<'a, 'id, T, N> {
    pub fn vacancy(&mut self) -> Option<Slot<'_, 'a, 'id, T, N>> {
        let slot = self.len.vacancy()?;
        Some(Slot { prefix: self, slot })
    }

    pub fn as_slice(&self) -> &[T] {
        // SAFETY: only the counted prefix is initialized and owned by this guard.
        unsafe { self.len.prefix(self.entries).assume_init_ref() }
    }

    /// Retains the initialized prefix's fixed capacity when projecting another array.
    pub const fn extent(&self) -> array::Len<N> {
        self.len
    }

    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the exclusive loan preserves every initialized position.
        unsafe { self.len.prefix_mut(self.entries).assume_init_mut() }
    }

    /// Borrows the initialized position issued by this exact prefix.
    pub fn get(&self, key: capacity::Key<'id, u8>) -> &T {
        // SAFETY: insertion alone issues this hidden domain's keys, and no
        // operation removes an initialized position during the scope.
        unsafe {
            self.entries
                .get_unchecked(*key.as_inner() as usize)
                .assume_init_ref()
        }
    }

    pub fn get_mut(&mut self, key: capacity::Key<'id, u8>) -> &mut T {
        // SAFETY: the same initialized position remains exclusively borrowed.
        unsafe {
            self.entries
                .get_unchecked_mut(*key.as_inner() as usize)
                .assume_init_mut()
        }
    }
}

impl<'id, T, const N: usize> Slot<'_, '_, 'id, T, N> {
    /// Initializes the admitted position before issuing its retained key.
    pub fn insert(self, value: T) -> capacity::Key<'id, u8> {
        let index = self.slot.index() as u8;
        self.slot.get_mut(self.prefix.entries).write(value);
        self.prefix.len = self.slot.commit();
        // SAFETY: this scope's sole prefix now owns the initialized position.
        capacity::Key::new(unsafe { crate::depot::raw::Proof::new(index) })
    }
}

impl<T, const N: usize> Drop for Prefix<'_, '_, T, N> {
    fn drop(&mut self) {
        crate::depot::raw::Drain::destroy::<false>(self);
    }
}

// SAFETY: the prefix retains exclusive backing and detaches its initialized
// positions once, before any element destruction can run.
unsafe impl<T, const N: usize> crate::depot::raw::Drain<T> for Prefix<'_, '_, T, N> {
    type Slots = [mem::MaybeUninit<T>; N];

    fn detach(&mut self) -> (&mut Self::Slots, ops::Range<usize>) {
        let len = mem::replace(&mut self.len, array::Len::ZERO);
        (self.entries, 0..len.get())
    }
}

/// Exclusive initialization of one caller-owned array.
#[must_use = "dropping a guard releases its initialized prefix"]
pub struct Guard<'a, T, const N: usize, const REVERSE: bool = false, P = ()> {
    entries: &'a mut [mem::MaybeUninit<T>; N],
    /// Initialized prefix length, advanced only by an admitted vacancy.
    len: usize,
    phase: marker::PhantomData<P>,
}

/// Marks a guard with an initialized prefix and an exact next uninitialized position.
#[derive(Debug)]
pub struct Vacant;

/// Marks a guard whose complete array is initialized.
#[derive(Debug)]
pub struct Full;

/// Owns the unread initialized prefix without moving its backing array.
#[derive(Debug)]
#[repr(transparent)]
#[must_use = "dropping the iterator releases its unread values"]
pub struct IntoIter<'a, T, const REVERSE: bool = false> {
    entries: &'a mut [mem::MaybeUninit<T>],
}

impl<'a, T, const N: usize, const REVERSE: bool> Guard<'a, T, N, REVERSE> {
    /// Initializes in array order; REVERSE selects reverse release of an abandoned prefix.
    pub fn new(entries: &'a mut mem::MaybeUninit<[T; N]>) -> Self {
        // SAFETY: MaybeUninit<[T; N]> has the same layout as [MaybeUninit<T>; N],
        // and this guard retains its exclusive borrow without publishing any T.
        let entries = unsafe { &mut *entries.as_mut_ptr().cast() };
        Self {
            entries,
            len: 0,
            phase: marker::PhantomData,
        }
    }

    /// Maps an admitted source into at most one value per initialized position.
    /// On failure, the caller retains the initialized prefix for rollback.
    pub fn try_map<'source, U, V, E>(
        entries: &'a mut [mem::MaybeUninit<U>; N],
        source: &'source Prefix<'_, '_, V, N>,
        mut map: impl FnMut(&'source V) -> Result<Option<T>, E>,
    ) -> Result<Self, (Self, E)> {
        let mut guard = Self {
            entries: crate::mem::raw::cast_uninit(entries),
            len: 0,
            phase: marker::PhantomData,
        };
        for value in source.as_slice() {
            let value = match map(value) {
                Ok(value) => value,
                Err(error) => return Err((guard, error)),
            };
            if let Some(value) = value {
                // SAFETY: each earlier source position inserted at most one
                // value, so this next source position proves a vacancy below N.
                guard = guard.phase::<Vacant>().insert(value);
            }
        }
        Ok(guard)
    }

    /// Completes the array or transfers initialization to its next position.
    pub fn next(
        self,
    ) -> ops::ControlFlow<Guard<'a, T, N, REVERSE, Full>, Guard<'a, T, N, REVERSE, Vacant>> {
        if self.len < N {
            return ops::ControlFlow::Continue(self.phase());
        }
        ops::ControlFlow::Break(self.phase())
    }
}

impl<T, const N: usize, const REVERSE: bool> Guard<'_, T, N, REVERSE, Full> {
    pub fn into_array(self) -> [T; N] {
        let this = mem::ManuallyDrop::new(self);
        unsafe { this.entries.as_ptr().cast::<[T; N]>().read() }
    }

    /// Consumes matching initialized positions once, without moving the full source array.
    pub fn map_with<U, V>(self, values: [U; N], mut map: impl FnMut(T, U) -> V) -> [V; N] {
        let this = mem::ManuallyDrop::new(self);
        let mut index = 0;
        values.map(|other| {
            // SAFETY: array::map visits exactly N positions in order, and Full owns every T.
            let value = unsafe { this.entries.get_unchecked(index).assume_init_read() };
            index += 1;
            map(value, other)
        })
    }
}

impl<'a, T, const N: usize, const REVERSE: bool> Guard<'a, T, N, REVERSE, Vacant> {
    /// Selects the same bounded position in another complete array.
    pub fn get<'b, U>(&self, values: &'b [U; N]) -> &'b U {
        // SAFETY: Guard::next proves this vacancy's position is strictly below N.
        unsafe { values.get_unchecked(self.len) }
    }

    /// Retains exclusive access to the same bounded position.
    pub fn get_mut<'b, U>(&self, values: &'b mut [U; N]) -> &'b mut U {
        // SAFETY: this vacancy retains the exact N bound established by Guard::next.
        unsafe { values.get_unchecked_mut(self.len) }
    }

    /// Pairs the initialized prefix with the same positions in a complete array.
    pub fn zip<'b, 'c, U>(
        &'b self,
        values: &'c [U; N],
    ) -> iter::Zip<slice::Iter<'b, T>, slice::Iter<'c, U>> {
        let len = self.len;
        // SAFETY: the complete array contains this guard's bounded prefix.
        let values = unsafe { values.get_unchecked(..len) };
        self.as_slice().iter().zip(values)
    }

    /// Initializes this position once and returns the extended prefix.
    pub fn insert(mut self, value: T) -> Guard<'a, T, N, REVERSE> {
        // SAFETY: the owned vacancy proves len < N, so insertion cannot overflow.
        unsafe { self.entries.get_unchecked_mut(self.len).write(value) };
        self.len += 1;
        self.phase()
    }
}

impl<T, const N: usize, const REVERSE: bool, P> Drop for Guard<'_, T, N, REVERSE, P> {
    fn drop(&mut self) {
        crate::depot::raw::Drain::destroy::<REVERSE>(self);
    }
}

const _: () = {
    assert!(size_of::<Prefix<'static, 'static, u8, 256>>() == 2 * size_of::<usize>());
    assert!(size_of::<Guard<'static, u8, 1>>() == 2 * size_of::<usize>());
    assert!(size_of::<Guard<'static, u8, 1, false, Vacant>>() == 2 * size_of::<usize>());
    assert!(size_of::<Guard<'static, u8, 1, false, Full>>() == 2 * size_of::<usize>());
    assert!(size_of::<IntoIter<'static, u8>>() == 2 * size_of::<usize>());
};

impl<'a, T, const N: usize, const REVERSE: bool, P> Guard<'a, T, N, REVERSE, P> {
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: this guard owns exactly the initialized prefix, whose
        // destruction or consumption cannot overlap this shared borrow.
        unsafe { slice::from_raw_parts(self.entries.as_ptr().cast(), self.len) }
    }

    fn into_parts(self) -> (&'a mut [mem::MaybeUninit<T>; N], usize) {
        let this = mem::ManuallyDrop::new(self);
        // SAFETY: consuming this guard transfers its exclusive initialized prefix once.
        let entries = unsafe { ptr::read(&this.entries) };
        (entries, this.len)
    }

    fn phase<Q>(self) -> Guard<'a, T, N, REVERSE, Q> {
        let (entries, len) = self.into_parts();
        Guard {
            entries,
            len,
            phase: marker::PhantomData,
        }
    }
}

impl<'a, T, const N: usize, const REVERSE: bool, P> IntoIterator for Guard<'a, T, N, REVERSE, P> {
    type Item = T;
    type IntoIter = IntoIter<'a, T, REVERSE>;

    fn into_iter(self) -> Self::IntoIter {
        let (entries, len) = self.into_parts();
        // SAFETY: the consumed guard owns exactly this initialized, bounded prefix.
        let entries = unsafe { entries.get_unchecked_mut(..len) };
        IntoIter { entries }
    }
}

impl<'a, T> IntoIter<'a, T> {
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: the iterator exclusively owns every still-unread initialized value.
        // Advancing and destruction are excluded for this short borrow.
        unsafe { self.entries.assume_init_mut() }
    }

    pub(super) fn drain<const N: usize>(source: &'a mut prefix::Prefix<T, N>) -> Self {
        let len = mem::replace(&mut source.len, array::Len::ZERO);
        Self {
            entries: len.prefix_mut(&mut source.entries),
        }
    }
}

impl<T, const REVERSE: bool> Iterator for IntoIter<'_, T, REVERSE> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let entries = mem::take(&mut self.entries);
        let (entry, rest) = entries.split_first_mut()?;
        self.entries = rest;
        // SAFETY: advancement transfers this initialized position exactly once.
        Some(unsafe { entry.assume_init_read() })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.entries.len();
        (len, Some(len))
    }
}

impl<T, const REVERSE: bool> ExactSizeIterator for IntoIter<'_, T, REVERSE> {}

impl<T, const REVERSE: bool> iter::FusedIterator for IntoIter<'_, T, REVERSE> {}

impl<T, const REVERSE: bool> Drop for IntoIter<'_, T, REVERSE> {
    fn drop(&mut self) {
        crate::depot::raw::Drain::destroy::<REVERSE>(self);
    }
}

// SAFETY: the iterator owns its exact unread suffix and detachment exhausts it.
unsafe impl<T, const REVERSE: bool> crate::depot::raw::Drain<T> for IntoIter<'_, T, REVERSE> {
    type Slots = [mem::MaybeUninit<T>];

    fn detach(&mut self) -> (&mut Self::Slots, ops::Range<usize>) {
        let entries = mem::take(&mut self.entries);
        let len = entries.len();
        (entries, 0..len)
    }
}

// SAFETY: the guard retains exclusive backing and initializes only its counted prefix.
unsafe impl<T, const N: usize, const REVERSE: bool, P> crate::depot::raw::Drain<T>
    for Guard<'_, T, N, REVERSE, P>
{
    type Slots = [mem::MaybeUninit<T>; N];

    fn detach(&mut self) -> (&mut Self::Slots, ops::Range<usize>) {
        let len = self.len;
        self.len = 0;
        (self.entries, 0..len)
    }
}

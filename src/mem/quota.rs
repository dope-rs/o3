use std::{cell, marker};

/// A single-threaded counter from which refundable reservations are drawn.
#[repr(transparent)]
pub struct Ledger<Tag = ()> {
    remaining: cell::Cell<usize>,
    tag: marker::PhantomData<fn(Tag) -> Tag>,
    thread: crate::ThreadBound,
}

/// A reservation which can split exclusive child leases through shared access.
#[must_use = "unused quota is returned when the shared reservation is dropped"]
#[repr(C)]
pub struct Shared<'source, Tag = ()> {
    source: &'source cell::Cell<usize>,
    remaining: Ledger<Tag>,
}

/// An exclusive consumable reservation.
#[must_use = "unused quota is returned when the lease is dropped"]
#[repr(C)]
pub struct Lease<'source, Tag = ()> {
    source: &'source cell::Cell<usize>,
    remaining: usize,
    tag: marker::PhantomData<fn(Tag) -> Tag>,
}

/// Linear proof that one unit was consumed from an exclusively borrowed lease.
pub struct Permit<'lease>(marker::PhantomData<&'lease mut ()>);

/// The result of conditionally admitting one item under quota.
#[must_use = "admission reports whether work was acquired or quota was exhausted"]
pub enum Admission<T> {
    Item(T),
    Empty,
    Exhausted,
}

impl<Tag> Ledger<Tag> {
    pub const fn new(remaining: usize) -> Self {
        Self {
            remaining: cell::Cell::new(remaining),
            tag: marker::PhantomData,
            thread: crate::ThreadBound::NEW,
        }
    }

    #[inline]
    pub fn remaining(&self) -> usize {
        self.remaining.get()
    }

    /// Replaces the available quota after every reservation has ended.
    #[inline]
    pub fn reset(&mut self, remaining: usize) {
        self.remaining.set(remaining);
    }

    #[inline]
    pub fn take(&self) -> bool {
        let Some(remaining) = self.remaining.get().checked_sub(1) else {
            return false;
        };
        self.remaining.set(remaining);
        true
    }

    pub fn admit_with<T>(&self, acquire: impl FnOnce() -> Option<T>) -> Admission<T> {
        let remaining = self.remaining.get();
        if remaining == 0 {
            return Admission::Exhausted;
        }
        self.remaining.set(remaining - 1);
        let Some(value) = acquire() else {
            self.remaining.set(self.remaining.get() + 1);
            return Admission::Empty;
        };
        Admission::Item(value)
    }

    fn counter(&self) -> &cell::Cell<usize> {
        &self.remaining
    }
}

impl<'source, Tag> Shared<'source, Tag> {
    #[inline]
    pub fn reserve_exact<SourceTag>(
        source: &'source Ledger<SourceTag>,
        count: usize,
    ) -> Option<Self> {
        Some(Self {
            source: source.counter(),
            remaining: Ledger::new(reserve_exact(source.counter(), count)?),
        })
    }

    #[inline]
    pub fn reserve_up_to<SourceTag>(source: &'source Ledger<SourceTag>, limit: usize) -> Self {
        Self {
            source: source.counter(),
            remaining: Ledger::new(reserve_up_to(source.counter(), limit)),
        }
    }

    #[inline]
    pub fn reserve_all<SourceTag>(source: &'source Ledger<SourceTag>) -> Self {
        Self::reserve_up_to(source, usize::MAX)
    }

    #[inline]
    pub fn lease_exact<ChildTag>(&self, count: usize) -> Option<Lease<'_, ChildTag>> {
        Lease::reserve_exact(&self.remaining, count)
    }

    #[inline]
    pub fn lease_up_to<ChildTag>(&self, limit: usize) -> Lease<'_, ChildTag> {
        Lease::reserve_up_to(&self.remaining, limit)
    }

    #[inline]
    pub fn lease_all<ChildTag>(&self) -> Lease<'_, ChildTag> {
        Lease::reserve_all(&self.remaining)
    }

    #[inline]
    pub fn remaining(&self) -> usize {
        self.remaining.remaining()
    }

    #[inline]
    pub fn spend(&mut self, count: usize) {
        let remaining = self.remaining.remaining();
        assert!(count <= remaining, "quota consumption exceeds reservation");
        self.remaining.reset(remaining - count);
    }
}

impl<'source, Tag> Lease<'source, Tag> {
    /// Reserves an exclusive lease tied to `source`.
    ///
    /// A child lease is similarly tied to the borrow of its shared parent:
    ///
    /// ```compile_fail
    /// use o3::mem::quota::{Lease, Shared};
    ///
    /// fn widen<'parent, 'child>(parent: &'child Shared<'parent>) -> Lease<'parent> {
    ///     parent.lease_all()
    /// }
    /// ```
    #[inline]
    pub fn reserve_exact<SourceTag>(
        source: &'source Ledger<SourceTag>,
        count: usize,
    ) -> Option<Self> {
        Some(Self {
            source: source.counter(),
            remaining: reserve_exact(source.counter(), count)?,
            tag: marker::PhantomData,
        })
    }

    #[inline]
    pub fn reserve_up_to<SourceTag>(source: &'source Ledger<SourceTag>, limit: usize) -> Self {
        Self {
            source: source.counter(),
            remaining: reserve_up_to(source.counter(), limit),
            tag: marker::PhantomData,
        }
    }

    #[inline]
    pub fn reserve_all<SourceTag>(source: &'source Ledger<SourceTag>) -> Self {
        Self::reserve_up_to(source, usize::MAX)
    }

    #[inline]
    pub const fn remaining(&self) -> usize {
        self.remaining
    }

    #[inline]
    pub fn spend(&mut self, count: usize) {
        assert!(
            count <= self.remaining,
            "quota consumption exceeds reservation"
        );
        self.remaining -= count;
    }

    #[inline]
    pub fn take(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }

    /// Consumes one unit and keeps the lease mutably borrowed while the returned
    /// proof is live.
    ///
    /// ```compile_fail
    /// use o3::mem::quota::{Lease, Ledger};
    ///
    /// let ledger = Ledger::new(2);
    /// let mut lease = Lease::reserve_all(&ledger);
    /// let permit = lease.take_permit().unwrap();
    /// let _ = lease.take();
    /// drop(permit);
    /// ```
    #[inline]
    pub fn take_permit(&mut self) -> Option<Permit<'_>> {
        self.take().then_some(Permit(marker::PhantomData))
    }

    #[inline]
    pub fn admit_with<T>(&mut self, acquire: impl FnOnce() -> Option<T>) -> Admission<T> {
        if self.remaining == 0 {
            return Admission::Exhausted;
        }
        let Some(value) = acquire() else {
            return Admission::Empty;
        };
        self.remaining -= 1;
        Admission::Item(value)
    }
}

impl<Tag> Drop for Shared<'_, Tag> {
    fn drop(&mut self) {
        refund(self.source, self.remaining.remaining());
    }
}

impl<Tag> Drop for Lease<'_, Tag> {
    fn drop(&mut self) {
        refund(self.source, self.remaining);
    }
}

fn reserve_exact(source: &cell::Cell<usize>, count: usize) -> Option<usize> {
    let available = source.get();
    let remaining = available.checked_sub(count)?;
    if count != 0 {
        source.set(remaining);
    }
    Some(count)
}

fn reserve_up_to(source: &cell::Cell<usize>, limit: usize) -> usize {
    let available = source.get();
    let reserved = available.min(limit);
    if reserved != 0 {
        source.set(available - reserved);
    }
    reserved
}

fn refund(source: &cell::Cell<usize>, unused: usize) {
    if unused != 0 {
        source.set(source.get() + unused);
    }
}

const _: () = {
    use std::mem;

    assert!(mem::size_of::<Ledger>() == mem::size_of::<usize>());
    assert!(mem::align_of::<Ledger>() == mem::align_of::<usize>());
    assert!(mem::size_of::<Shared<'static>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<'static>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Admission<usize>>() == 2 * mem::size_of::<usize>());
};

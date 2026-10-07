use std::marker;

use crate::{base::cell::region, mem::lease};

/// A single-threaded counter from which refundable reservations are drawn.
#[repr(transparent)]
pub struct Ledger<Tag = ()> {
    remaining: crate::mem::raw::Account,
    tag: marker::PhantomData<fn(Tag) -> Tag>,
}

/// Returns one unit to the exact ledger which created its lease.
#[repr(transparent)]
pub struct Return<'source, Tag = ()> {
    source: &'source crate::mem::raw::Account,
    tag: marker::PhantomData<fn(Tag) -> Tag>,
}

/// A reservation which can split exclusive child leases through shared access.
#[must_use = "unused quota is returned when the shared reservation is dropped"]
#[repr(C)]
pub struct Shared<'source, Tag = (), M = lease::Sealed> {
    source: &'source crate::mem::raw::Account,
    remaining: Ledger<Tag>,
    mode: marker::PhantomData<M>,
}

/// An exclusive consumable reservation.
#[must_use = "unused quota is returned when the lease is dropped"]
#[repr(C)]
pub struct Lease<'source, Tag = ()> {
    source: &'source crate::mem::raw::Account,
    remaining: usize,
    tag: marker::PhantomData<fn(Tag) -> Tag>,
}

/// The result of conditionally admitting one item under quota.
#[must_use = "admission reports whether work was acquired or quota was exhausted"]
pub enum Admission<T> {
    Item(T),
    Empty,
    Exhausted,
}

/// Whether an attempted quota acquisition produced one item.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Acquire {
    Item,
    Empty,
}

#[must_use = "take reports whether the requested quota was consumed"]
pub enum Take {
    Taken,
    Exhausted,
}

impl<Tag> Ledger<Tag> {
    pub const fn new(left: usize) -> Self {
        Self {
            remaining: crate::mem::raw::Account::new(left),
            tag: marker::PhantomData,
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining.get()
    }

    /// Replaces the available quota after every reservation has ended.
    pub fn reset(&mut self, left: usize) {
        self.remaining = crate::mem::raw::Account::new(left);
    }

    pub fn take(&self) -> Take {
        self.remaining.reserve(1)
    }

    /// Reserves one refundable unit whose owner may consume its parts.
    pub fn open(&self) -> Option<crate::mem::Lease<Return<'_, Tag>, lease::Open>> {
        Some(crate::mem::Lease::open(self.reserve()?, ()))
    }

    fn reserve(&self) -> Option<Return<'_, Tag>> {
        if let Take::Exhausted = self.take() {
            return None;
        }
        Some(Return {
            source: &self.remaining,
            tag: marker::PhantomData,
        })
    }

    pub fn admit_with<T>(&self, acquire: impl FnOnce() -> Option<T>) -> Admission<T> {
        let Some(reserved) = self.reserve() else {
            return Admission::Exhausted;
        };
        let Some(value) = acquire() else {
            crate::mem::Return::return_item(reserved, ());
            return Admission::Empty;
        };
        Admission::Item(value)
    }

    pub fn apply(&self, run: impl FnOnce() -> Acquire) {
        let _ = self.admit_with(|| (run() == Acquire::Item).then_some(()));
    }
}

impl<'source, Tag, M> Shared<'source, Tag, M> {
    pub fn reserve<SourceTag>(source: &'source Ledger<SourceTag>, count: usize) -> Option<Self> {
        let source = &source.remaining;
        if let Take::Exhausted = source.reserve(count) {
            return None;
        }
        Some(Self {
            source,
            remaining: Ledger::new(count),
            mode: marker::PhantomData,
        })
    }

    pub fn reserve_up<SourceTag>(source: &'source Ledger<SourceTag>, limit: usize) -> Self {
        Self {
            source: &source.remaining,
            remaining: Ledger::new(source.remaining.reserve_up(limit)),
            mode: marker::PhantomData,
        }
    }

    pub fn reserve_all<SourceTag>(source: &'source Ledger<SourceTag>) -> Self {
        Self::reserve_up(source, usize::MAX)
    }

    pub fn lease_up_to<ChildTag>(&self, limit: usize) -> Lease<'_, ChildTag> {
        Lease::reserve_up(&self.remaining, limit)
    }

    pub fn remaining(&self) -> usize {
        self.remaining.remaining()
    }

    /// Returns this reservation's unused quota after every child lease has ended.
    pub fn refund(self) {
        drop(self);
    }
}

impl<Tag> Shared<'_, Tag, lease::Open> {
    /// Settles only the unused proof issued inside this reservation's scope.
    pub fn with<C, R>(
        self,
        context: C,
        run: impl for<'id> FnOnce(
            C,
            region::Token<region::Identity<'id>>,
        ) -> (R, crate::mem::raw::Unused<'id>),
    ) -> R {
        region::Token::scope(|token| {
            let reserved = token.bind(self);
            let (output, unused) = run(context, token);
            let (amount, mut token) = unused.into_inner();
            let reserved = token.borrow_mut(&reserved);
            reserved.remaining.reset(amount);
            output
        })
    }
}

impl<'source, Tag> Lease<'source, Tag> {
    /// Reserves an exclusive lease tied to `source`.
    ///
    /// A child lease is similarly tied to the borrow of its shared parent.
    pub fn reserve_up<SourceTag>(source: &'source Ledger<SourceTag>, limit: usize) -> Self {
        Self {
            source: &source.remaining,
            remaining: source.remaining.reserve_up(limit),
            tag: marker::PhantomData,
        }
    }

    pub fn reserve_all<SourceTag>(source: &'source Ledger<SourceTag>) -> Self {
        Self::reserve_up(source, usize::MAX)
    }

    pub const fn remaining(&self) -> usize {
        self.remaining
    }

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

impl<Tag> Drop for Lease<'_, Tag> {
    fn drop(&mut self) {
        self.source.refund_spare(self.remaining);
    }
}

impl<Tag, M> Drop for Shared<'_, Tag, M> {
    fn drop(&mut self) {
        self.source.refund_spare(self.remaining.remaining());
    }
}

impl<Tag> crate::mem::Return for Return<'_, Tag> {
    type Item = ();

    fn return_item(self, (): Self::Item) {
        self.source.refund(1);
    }
}

const _: () = {
    use std::mem;

    assert!(mem::size_of::<Ledger>() == mem::size_of::<usize>());
    assert!(mem::align_of::<Ledger>() == mem::align_of::<usize>());
    assert!(mem::size_of::<Return<'static>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<crate::mem::Lease<Return<'static>>>() == mem::size_of::<usize>());
    assert!(
        mem::size_of::<Option<crate::mem::Lease<Return<'static>>>>() == mem::size_of::<usize>()
    );
    assert!(mem::size_of::<Shared<'static>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Shared<'static, (), lease::Open>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Lease<'static>>() == 2 * mem::size_of::<usize>());
    assert!(mem::size_of::<Admission<usize>>() == 2 * mem::size_of::<usize>());
};

use std::marker;

use crate::{
    depot,
    depot::{
        array::copy,
        index,
        index::{bitmap::entry::Row, set},
    },
};

/// Retained ownership carrying exactly the predicate named by P.
#[repr(transparent)]
pub struct Proof<S, P: depot::Predicate> {
    value: S,
    invariant: Domain<S, P>,
}

// SAFETY: the retained initialized-prefix proof names this exact borrowed vector.
unsafe impl<'a, T: Copy, const N: usize> depot::raw::Project<usize> for &'a copy::Vec<T, N> {
    type Output = &'a T;

    unsafe fn project(self, index: usize) -> Self::Output {
        // SAFETY: Project requires an initialized index in this retained source.
        unsafe { self.as_slice().get_unchecked(index) }
    }
}

// SAFETY: projection retains the initialized row's exact set, domain and borrow.
unsafe impl<Tag, Raw: depot::raw::Word, const N: usize>
    depot::raw::Project<index::Ordinal<Tag, Raw>> for &set::State<Tag, Raw, N>
{
    type Output = set::Entry<Self, Tag, Raw, N>;

    unsafe fn project(self, index: index::Ordinal<Tag, Raw>) -> Self::Output {
        // SAFETY: Project admits this row in this same initialized set; no membership is implied.
        let row = unsafe { Row::new(set::View::new(self), index.offset()) };
        set::Entry { row }
    }
}

impl<S, P: depot::Predicate> Proof<S, P> {
    /// # Safety
    /// P holds until its authorized consuming transition; ownership and lifetime obligations
    /// prescribed by P remain valid for every derived output.
    pub const unsafe fn new(value: S) -> Self {
        Self {
            value,
            invariant: marker::PhantomData,
        }
    }

    /// # Safety
    /// The returned value preserves the complete predicate P and its exact retained authority.
    pub unsafe fn map<T>(self, project: impl FnOnce(S) -> T) -> Proof<T, P> {
        Proof {
            value: project(self.value),
            invariant: marker::PhantomData,
        }
    }

    pub fn into_inner(self) -> S {
        self.value
    }
}

impl<S, K, P> Proof<(S, K), index::Initialized<P>> {
    pub const fn source(&self) -> &S {
        &self.value.0
    }
    pub const fn key(&self) -> &K {
        &self.value.1
    }

    /// # Safety
    /// Sharing the retained source preserves every P obligation and grants no new exclusive authority.
    pub unsafe fn as_ref(&self) -> Proof<(&S, K), index::Initialized<P>>
    where
        K: Copy,
    {
        Proof {
            value: (&self.value.0, self.value.1),
            invariant: marker::PhantomData,
        }
    }

    /// # Safety
    /// The borrowed operation preserves initialization and every ownership and state condition of P.
    pub unsafe fn as_mut(&mut self) -> Proof<(&mut S, K), index::Initialized<P>>
    where
        K: Copy,
    {
        Proof {
            value: (&mut self.value.0, self.value.1),
            invariant: marker::PhantomData,
        }
    }

    pub fn into(self) -> S::Output
    where
        S: depot::raw::Project<K, P>,
    {
        // SAFETY: this exact initialized phase retains every Project ownership obligation.
        unsafe { self.value.0.project(self.value.1) }
    }
}

impl<'a, T> depot::raw::Admit for Proof<(&'a mut Vec<T>, usize), index::Initialized<()>> {
    type Output = (T, Option<&'a T>);

    fn admit(self) -> Self::Output {
        let (values, index) = self.into_inner();
        // SAFETY: this consuming proof retains the initialized index in the same vector.
        unsafe { crate::alloc::raw::VecExt::swap_remove(values, index) }
    }
}

const _: () = {
    assert!(
        std::mem::size_of::<Proof<(&'static (), usize), index::Initialized<()>>>()
            == 2 * std::mem::size_of::<usize>()
    );
    assert!(
        std::mem::size_of::<Option<Proof<(&'static (), usize), index::Initialized<()>>>>()
            == 2 * std::mem::size_of::<usize>()
    );
};

mod phases;
pub(in crate::depot) use phases::{Numeric, Sealed};

mod slot;
mod values;
pub(in crate::depot) use drain::Drain;
pub(crate) use slot::{Present, Slot};

mod count;
pub(in crate::depot) use count::ChainCount;

mod drain;

type Domain<S, P> = marker::PhantomData<fn(S, P) -> (S, P)>;

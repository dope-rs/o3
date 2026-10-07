//! Fixed-capacity locally shared immutable-value pool.

use std::{cell, mem};

use crate::depot::{
    arena::pool::{self, recycle},
    capacity,
};

#[derive(Debug)]
pub struct Resident<T> {
    pub(in crate::depot) references: crate::mem::raw::Count,
    pub(super) value: cell::UnsafeCell<mem::MaybeUninit<T>>,
}

pub type Header<T> = recycle::Header<Resident<T>>;

/// Fixed storage for locally shared immutable values.
#[repr(transparent)]
pub struct Store<T> {
    block: recycle::Store<Resident<T>>,
}

#[derive(Debug)]
pub struct Initialized;

impl<T> Resident<T> {
    const fn new() -> Self {
        Self {
            references: crate::mem::raw::Count::empty(),
            value: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
        }
    }
}

impl<T> Store<T> {
    pub fn try_new(capacity: capacity::Count) -> Result<Self, crate::alloc::Error> {
        Ok(Self {
            block: recycle::Store::try_from_fn(capacity, |_| Resident::new())?,
        })
    }

    pub fn vacancies(&self) -> &Header<T> {
        self.block.vacancies()
    }

    pub fn vacancy(&self) -> Option<pool::Vacant<'_, Resident<T>, Header<T>>> {
        self.vacancies().vacancy()
    }
}

impl<T> Header<T> {
    pub fn vacancy(&self) -> Option<pool::Vacant<'_, Resident<T>, Header<T>>> {
        pool::Vacant::new(self)
    }
}

const _: () = {
    assert!(mem::size_of::<Store<()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<&'static Header<()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<&'static Header<()>>>() == mem::size_of::<usize>());
    assert!(
        mem::size_of::<pool::Vacant<'static, Resident<()>, Header<()>>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<pool::Vacant<'static, Resident<()>, Header<()>>>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<pool::Unique<'static, Resident<()>, Header<()>>>()
            == mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<pool::Unique<'static, Resident<()>, Header<()>, Initialized>>()
            == mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<pool::Unique<'static, Resident<()>, Header<()>, Initialized>>>()
            == mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<pool::Shared<'static, Resident<()>, Header<()>, Initialized>>()
            == mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<pool::Shared<'static, Resident<()>, Header<()>, Initialized>>>()
            == mem::size_of::<usize>()
    );
};

// SAFETY: this policy is installed only after the resident value is initialized.
unsafe impl<T> crate::depot::raw::PoolPolicy<Resident<T>, Header<T>> for Initialized {
    fn retire(mut entry: pool::Terminal<'_, Resident<T>, Header<T>>) {
        // SAFETY: this policy is installed only after the resident value is initialized.
        unsafe { (*entry.get_mut().value.get()).assume_init_drop() };
        entry.recycle();
    }
}

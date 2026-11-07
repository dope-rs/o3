//! Fixed-capacity locally shared immutable-value pool.

use std::{cell, mem};

use crate::depot::arena::pool::{self, recycle};

#[derive(Debug)]
pub struct Resident<T> {
    pub(in crate::depot) references: crate::mem::raw::Count,
    pub(super) value: cell::UnsafeCell<mem::MaybeUninit<T>>,
}

pub type Header<T> = recycle::Header<Resident<T>>;

/// Fixed storage for locally shared immutable values.
pub type Store<T> = recycle::Store<Resident<T>>;

#[derive(Debug)]
pub struct Initialized;

impl<T> Default for Resident<T> {
    fn default() -> Self {
        Self {
            references: crate::mem::raw::Count::empty(),
            value: cell::UnsafeCell::new(mem::MaybeUninit::uninit()),
        }
    }
}

impl<T> Store<T> {
    pub fn vacancy(&self) -> Option<pool::Vacant<'_, Resident<T>>> {
        self.vacancies().vacancy()
    }
}

impl<T> Header<T> {
    pub fn vacancy(&self) -> Option<pool::Vacant<'_, Resident<T>>> {
        pool::Vacant::new(self)
    }
}

const _: () = {
    assert!(mem::size_of::<Store<()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<&'static Header<()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<&'static Header<()>>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<pool::Vacant<'static, Resident<()>>>() == 2 * mem::size_of::<usize>());
    assert!(
        mem::size_of::<Option<pool::Vacant<'static, Resident<()>>>>()
            == 2 * mem::size_of::<usize>()
    );
    assert!(mem::size_of::<pool::Unique<'static, Resident<()>>>() == mem::size_of::<usize>());
    assert!(
        mem::size_of::<pool::Unique<'static, Resident<()>, (), Initialized>>()
            == mem::size_of::<usize>()
    );
    assert!(
        mem::size_of::<Option<pool::Unique<'static, Resident<()>, (), Initialized>>>()
            == mem::size_of::<usize>()
    );
    assert!(mem::size_of::<pool::Shared<'static, ()>>() == mem::size_of::<usize>());
    assert!(mem::size_of::<Option<pool::Shared<'static, ()>>>() == mem::size_of::<usize>());
    assert!(
        mem::size_of::<Result<pool::Shared<'static, ()>, crate::mem::rc::Error>>()
            == mem::size_of::<pool::Shared<'static, ()>>()
    );
};

// SAFETY: this policy is installed only after the resident value is initialized.
unsafe impl<T> crate::depot::raw::PoolPolicy<Resident<T>, ()> for Initialized {
    fn retire(mut entry: pool::Terminal<'_, Resident<T>>) {
        // SAFETY: this policy is installed only after the resident value is initialized.
        unsafe { (*entry.get_mut().value.get()).assume_init_drop() };
        entry.recycle();
    }
}

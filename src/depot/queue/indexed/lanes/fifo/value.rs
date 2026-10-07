use std::{cell::UnsafeCell, mem};

use crate::depot::queue::{links, tagged};

pub struct Value<T> {
    pub(in crate::depot) hook: tagged::Hook,
    pub(in crate::depot) value: UnsafeCell<mem::MaybeUninit<T>>,
}

impl<T> Value<T> {
    pub(in crate::depot) fn vacant(index: links::Index) -> Self {
        Self {
            hook: tagged::Hook::new(index),
            value: UnsafeCell::new(mem::MaybeUninit::uninit()),
        }
    }
}

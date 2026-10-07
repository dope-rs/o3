use std::ptr;

use crate::buffer::{
    segments,
    segments::{Header, Pool},
};

pub trait Sealed {}
impl<T> Sealed for Pool<T> {}
impl<T> Sealed for &Header<T> {}

pub trait Handle<T>: Sealed {
    fn header(&self) -> &segments::Header<T>;

    fn is_same(&self, other: &Self) -> bool {
        ptr::eq(self.header(), other.header())
    }
}

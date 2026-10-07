use std::marker::PhantomData;

use crate::{buffer, buffer::segments};

pub struct Built<T: buffer::raw::Segment, S> {
    pub(in crate::buffer::segments) chain: segments::Chain<T>,
    source: PhantomData<fn() -> S>,
}

impl<T: buffer::raw::Segment, S> Built<T, S> {
    pub(in crate::buffer::segments::handles::pool::exact) fn new(
        chain: segments::Chain<T>,
    ) -> Self {
        Self {
            chain,
            source: PhantomData,
        }
    }
}

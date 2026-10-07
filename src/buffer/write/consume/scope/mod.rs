use std::{marker, mem, num};

pub use error::Error;

use crate::{buffer, buffer::storage};

mod error;

pub struct Brand<'id>(marker::PhantomData<fn(&'id mut ()) -> &'id mut ()>);

pub struct End<'id, const MAX: u32> {
    count: num::NonZero<u32>,
    brand: marker::PhantomData<Brand<'id>>,
}

impl<const MAX: u32> End<'_, MAX> {
    pub const fn count(&self) -> num::NonZero<u32> {
        self.count
    }
}

/// One source borrow; issuing range and consumption proofs needs no budget.
#[repr(transparent)]
pub struct Input<'input, R: buffer::Source, B = ()> {
    source: &'input R,
    brand: marker::PhantomData<B>,
}

impl<'input, R: buffer::Source> Input<'input, R> {
    /// Parses and consumes within one exclusive source borrow and fresh brand.
    pub fn consume<T, const MAX: u32>(
        source: &mut R,
        run: impl for<'id, 'a> FnOnce(Input<'a, R, Brand<'id>>) -> (Option<End<'id, MAX>>, T),
    ) -> T
    where
        R: buffer::Cursor + buffer::raw::Consume,
    {
        let (end, value) = run(Input {
            source,
            brand: marker::PhantomData,
        });
        if let Some(end) = end {
            R::consume(buffer::raw::Consumed::new(source, end));
        }
        value
    }

    pub const fn new(source: &'input R) -> Self {
        Self {
            source,
            brand: marker::PhantomData,
        }
    }

    /// Binds every end position to this exact input.
    pub fn scope<T>(self, run: impl for<'id> FnOnce(Input<'input, R, Brand<'id>>) -> T) -> T {
        run(Input {
            source: self.source,
            brand: marker::PhantomData,
        })
    }
}

impl<'input, R: buffer::Source, B> Input<'input, R, B> {
    pub(in crate::buffer) const fn source(&self) -> &'input R {
        self.source
    }
}

impl<'input, R: buffer::Cursor, B> Input<'input, R, B> {
    pub fn as_slice(&self) -> &'input [u8] {
        self.source.as_ref()
    }
}

impl<'id, 'input, R: buffer::Cursor> Input<'input, R, Brand<'id>> {
    pub fn try_prefix<const MAX: u32>(
        &self,
        count: num::NonZero<usize>,
    ) -> Result<(R::View<'input>, End<'id, MAX>), Error> {
        if count.get() > MAX as usize {
            return Err(Error::Maximum {
                count,
                maximum: MAX,
            });
        }
        let plan = self.source.try_get(0..count.get()).map_err(Error::Range)?;
        Ok((
            plan,
            End {
                count: num::NonZero::<u32>::MIN.saturating_add(count.get() as u32 - 1),
                brand: marker::PhantomData,
            },
        ))
    }
}

const _: () = {
    assert!(
        mem::size_of::<Input<'static, storage::Shared, Brand<'static>>>()
            == mem::size_of::<usize>()
    );
    assert!(mem::size_of::<End<'static, { u32::MAX }>>() == 4);
    assert!(mem::size_of::<Option<End<'static, { u32::MAX }>>>() == 4);
};

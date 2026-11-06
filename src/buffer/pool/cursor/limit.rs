use pool::state;

use crate::buffer::{pool, pool::owners, write::consume::Error};

/// A span and the remaining write budget admitted by that exact span.
pub struct Limit<T, B> {
    target: T,
    remaining: u32,
    brand: B,
}

impl<T: AsRef<[u8]>, B> AsRef<[u8]> for Limit<T, B> {
    fn as_ref(&self) -> &[u8] {
        self.target.as_ref()
    }
}

impl<T, B> Limit<T, B> {
    pub const fn remaining(&self) -> usize {
        self.remaining as usize
    }

    pub fn brand(&self) -> &B {
        &self.brand
    }

    pub fn into_parts(self) -> (T, B) {
        (self.target, self.brand)
    }
}

impl<L, M, B> Limit<pool::Span<owners::Plain<L>, M>, B> {
    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), Error> {
        let input = owners::Measured::slice(src);
        Error::check(input.len(), self.remaining as usize)?;
        self.remaining -= input.len() as u32;
        pool::Append {
            target: &mut self.target,
            input,
        }
        .commit();
        Ok(())
    }
}

impl<L, B> Limit<pool::Span<owners::Plain<L>>, B> {
    pub fn try_from(
        target: pool::Span<owners::Plain<L>>,
        bytes: usize,
        brand: B,
    ) -> Result<Self, (pool::Span<owners::Plain<L>>, B, Error)> {
        if let Err(error) = Error::check(bytes, target.capacity().get() as usize - target.len()) {
            return Err((target, brand, error));
        }
        Ok(Self {
            target,
            remaining: bytes as u32,
            brand,
        })
    }

    pub fn try_acquire(
        pool: &pool::Pool<state::Uninit, owners::Plain<L>>,
        bytes: usize,
        brand: B,
    ) -> Result<Self, (B, pool::Error)> {
        if let Err(error) = Error::check(bytes, pool.capacity().get() as usize) {
            return Err((brand, pool::Error::Capacity(error)));
        }
        let Some(target) = pool.acquire() else {
            return Err((brand, pool::Error::Empty));
        };
        Ok(Self {
            target,
            remaining: bytes as u32,
            brand,
        })
    }
}

impl<'pool, L, B> Limit<pool::Span<owners::Plain<L>, pool::Borrowed<'pool>>, B> {
    pub fn try_borrow(
        pool: &'pool pool::Pool<state::Uninit, owners::Plain<L>>,
        bytes: usize,
        brand: B,
    ) -> Result<Self, (B, pool::Error)> {
        if let Err(error) = Error::check(bytes, pool.capacity().get() as usize) {
            return Err((brand, pool::Error::Capacity(error)));
        }
        let Some(target) = pool.borrow() else {
            return Err((brand, pool::Error::Empty));
        };
        Ok(Self {
            target,
            remaining: bytes as u32,
            brand,
        })
    }
}

const _: () = {
    assert!(size_of::<Limit<pool::Span, u8>>() == size_of::<pool::Span>() + size_of::<usize>());
    assert!(size_of::<Option<Limit<pool::Span, u8>>>() == size_of::<Limit<pool::Span, u8>>());
};

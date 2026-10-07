use pool::state;

use crate::buffer::{pool, pool::owners, write::consume::Error};

/// A cursor and the remaining write budget admitted by that exact cursor.
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

impl<L, M, B> Limit<pool::Cursor<L, M>, B> {
    pub fn try_from(
        target: pool::Cursor<L, M>,
        bytes: usize,
        brand: B,
    ) -> Result<Self, (pool::Cursor<L, M>, B, Error)> {
        if let Err(error) = Error::check(bytes, target.spare_len()) {
            return Err((target, brand, error));
        }
        Ok(Self {
            target,
            remaining: bytes as u32,
            brand,
        })
    }

    pub fn try_extend(&mut self, src: &[u8]) -> Result<(), Error> {
        let input = owners::Measured::slice(src);
        Error::check(input.len(), self.remaining as usize)?;
        self.target.compact_for(input.len());
        self.remaining -= input.len() as u32;
        pool::Append {
            target: &mut self.target,
            input,
        }
        .commit();
        Ok(())
    }
}

impl<L, B> Limit<pool::Cursor<L>, B> {
    pub fn try_acquire(
        pool: &pool::Pool<state::Uninit, owners::Plain<L>>,
        bytes: usize,
        brand: B,
    ) -> Result<Self, (B, pool::Error)> {
        if let Err(error) = Error::check(bytes, pool.capacity().get() as usize) {
            return Err((brand, pool::Error::Capacity(error)));
        }
        let Some(target) = pool.cursor() else {
            return Err((brand, pool::Error::Empty));
        };
        Ok(Self {
            target,
            remaining: bytes as u32,
            brand,
        })
    }
}

impl<'pool, L, B> Limit<pool::Cursor<L, pool::Borrowed<'pool>>, B> {
    pub fn try_borrow(
        pool: &'pool pool::Pool<state::Uninit, owners::Plain<L>>,
        bytes: usize,
        brand: B,
    ) -> Result<Self, (B, pool::Error)> {
        if let Err(error) = Error::check(bytes, pool.capacity().get() as usize) {
            return Err((brand, pool::Error::Capacity(error)));
        }
        let Some(target) = pool.view() else {
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
    assert!(size_of::<Limit<pool::Cursor, u8>>() == size_of::<pool::Cursor>() + size_of::<usize>());
    assert!(size_of::<Option<Limit<pool::Cursor, u8>>>() == size_of::<Limit<pool::Cursor, u8>>());
};

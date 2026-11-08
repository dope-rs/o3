use crate::buffer::{
    pool,
    pool::{borrowed, state},
};

/// One checked initialized range for every acquisition from its original pool.
pub struct Source<'pool, const SUFFIX: usize = 0> {
    pool: &'pool pool::Pool<state::Initialized>,
    start: u32,
    end: u32,
}

impl<'pool, const SUFFIX: usize> Source<'pool, SUFFIX> {
    pub fn try_new(
        pool: &'pool pool::Pool<state::Initialized>,
        length: usize,
        suffix: usize,
    ) -> Result<Self, borrowed::Error> {
        let capacity = pool.capacity();
        let error = borrowed::Error::Capacity {
            length,
            suffix,
            capacity,
        };
        let end = (capacity.get() as usize).checked_sub(suffix).ok_or(error)?;
        let start = end.checked_sub(length).ok_or(error)?;
        if SUFFIX > length {
            return Err(borrowed::Error::Partition {
                suffix: SUFFIX,
                length,
            });
        }
        Ok(Self {
            pool,
            start: start as u32,
            end: end as u32,
        })
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        (self.end - self.start) as usize
    }

    pub fn acquire(&self) -> Option<borrowed::Slice<'pool>> {
        let span = pool::Span::borrow(&self.pool.core)?;
        Some(borrowed::Slice {
            span,
            start: self.start,
            end: self.end,
        })
    }

    pub fn acquire_frame(&self) -> Option<borrowed::Frame<'pool>> {
        let mut payload = self.acquire()?;
        payload.start = self.end - SUFFIX as u32;
        Some(borrowed::Frame {
            payload,
            start: self.start,
            end: self.end,
        })
    }
}

const _: () = assert!(size_of::<Source<'static>>() == 2 * size_of::<usize>());

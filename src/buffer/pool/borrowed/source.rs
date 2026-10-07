use crate::buffer::{
    pool,
    pool::{borrowed, state},
};

/// One checked initialized range for every acquisition from its original pool.
pub struct Source<'pool> {
    pool: &'pool pool::Pool<state::Initialized>,
    start: u32,
    end: u32,
}

impl<'pool> Source<'pool> {
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

    pub fn into_split(self) -> borrowed::Split<'pool> {
        borrowed::Split {
            source: self,
            suffix: 0,
        }
    }

    pub fn acquire(&self) -> Option<borrowed::Slice<'pool>> {
        let span = pool::Span::borrow(&self.pool.core)?;
        Some(borrowed::Slice {
            span,
            start: self.start,
            end: self.end,
        })
    }
}

const _: () = assert!(size_of::<Source<'static>>() == 2 * size_of::<usize>());

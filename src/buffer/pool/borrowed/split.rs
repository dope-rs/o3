use crate::buffer::pool::borrowed;

/// A suffix partition retained across every acquisition from one source.
pub struct Split<'pool> {
    pub(in crate::buffer::pool::borrowed) source: borrowed::Source<'pool>,
    pub(in crate::buffer::pool::borrowed) suffix: u32,
}

impl<'pool> Split<'pool> {
    pub fn try_suffix(mut self, suffix: usize) -> Result<Self, borrowed::Error> {
        let length = self.source.len();
        if suffix > length {
            return Err(borrowed::Error::Partition { suffix, length });
        }
        self.suffix = suffix as u32;
        Ok(self)
    }

    pub fn acquire(&self) -> Option<borrowed::Frame<'pool>> {
        let mut payload = self.source.acquire()?;
        let start = payload.start;
        let end = payload.end;
        payload.start = end - self.suffix;
        Some(borrowed::Frame {
            payload,
            start,
            end,
        })
    }
}

const _: () = assert!(size_of::<Split<'static>>() == 3 * size_of::<usize>());

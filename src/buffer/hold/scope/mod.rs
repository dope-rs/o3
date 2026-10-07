use std::{mem, num, ops};

pub use bound::Bound;
pub use deferred::Deferred;
use scope::{Brand, End};
use storage::range;

use crate::{
    buffer,
    buffer::{resident, storage, write::consume::scope},
};

mod bound;
mod deferred;

/// A source scope paired with its exact resident-accounting authority.
pub struct Input<'input, 'd, R: buffer::Source, B = ()> {
    source: scope::Input<'input, R, B>,
    budget: &'input resident::Budget<'d>,
}

impl<'input, 'd, R: buffer::Source, B> Input<'input, 'd, R, B> {
    /// Borrows this scope's original source and accounting authority.
    pub const fn as_parts(&self) -> (&'input R, &'input resident::Budget<'d>) {
        (self.source.source(), self.budget)
    }
}

impl<'input, 'd, R: buffer::Source> Input<'input, 'd, R> {
    pub fn consume<T, const MAX: u32>(
        source: &mut R,
        budget: &resident::Budget<'d>,
        run: impl for<'id, 'a> FnOnce(Input<'a, 'd, R, Brand<'id>>) -> (Option<End<'id, MAX>>, T),
    ) -> T
    where
        R: buffer::Cursor + buffer::raw::Consume,
    {
        scope::Input::consume(source, |source| run(Input { source, budget }))
    }

    pub const fn new(source: &'input R, budget: &'input resident::Budget<'d>) -> Self {
        Self {
            source: scope::Input::new(source),
            budget,
        }
    }

    pub fn scope<T>(self, run: impl for<'id> FnOnce(Input<'input, 'd, R, Brand<'id>>) -> T) -> T {
        let Self { source, budget } = self;
        source.scope(|source| run(Input { source, budget }))
    }
}

impl<'id, 'input, 'd, R: buffer::Cursor> Input<'input, 'd, R, Brand<'id>> {
    pub fn try_prefix<const MAX: u32>(
        &self,
        count: num::NonZero<usize>,
    ) -> Result<(Deferred<'input, 'd, R>, End<'id, MAX>), scope::Error> {
        let (plan, end) = self.source.try_prefix(count)?;
        Ok((
            Deferred {
                input: (plan, self.budget),
            },
            end,
        ))
    }
}

impl<'input, 'd, R: buffer::Cursor, B> Input<'input, 'd, R, B> {
    pub fn try_plan(
        &self,
        range: ops::Range<usize>,
    ) -> Result<Deferred<'input, 'd, R>, range::Error> {
        let plan = self.source.source().try_get(range)?;
        Ok(Deferred {
            input: (plan, self.budget),
        })
    }

    pub fn as_slice(&self) -> &'input [u8] {
        self.source.as_slice()
    }
}

impl<'input, 'd, R: buffer::Complete, B> Input<'input, 'd, R, B> {
    pub fn defer_all(&self) -> Deferred<'input, 'd, R> {
        Deferred {
            input: (self.source.source().view(), self.budget),
        }
    }
}

impl<'input, 'd, R: buffer::Cursor, B> ops::Deref for Input<'input, 'd, R, B> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.source.as_slice()
    }
}

impl<'input, 'd, R: buffer::Cursor, B> AsRef<[u8]> for Input<'input, 'd, R, B> {
    fn as_ref(&self) -> &[u8] {
        self.source.as_slice()
    }
}

const _: () = {
    let word = mem::size_of::<usize>();
    assert!(mem::size_of::<Deferred<'static, 'static, storage::Shared>>() == 4 * word);
    assert!(mem::size_of::<Input<'static, 'static, storage::Shared, Brand<'static>>>() == 2 * word);
};

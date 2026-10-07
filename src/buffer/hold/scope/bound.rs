use buffer::Read as _;
use hold::{Choice, scope::Deferred};

use crate::{
    buffer,
    buffer::{hold, resident, storage},
};

/// One deferred range with its retention policy and resident ceiling locked.
#[must_use]
pub struct Bound<'input, 'd, R: buffer::Source + 'input> {
    input: (R::View<'input>, &'input resident::Budget<'d>),
    choice: Choice,
}

impl<'input, 'd, R: buffer::Source + 'input> Bound<'input, 'd, R> {
    pub fn as_slice(&self) -> &'input [u8] {
        self.input.0.as_slice()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn defer(self) -> Deferred<'input, 'd, R> {
        Deferred { input: self.input }
    }
}

impl<'input, 'd, R: hold::raw::Retain<'d> + 'input> Bound<'input, 'd, R> {
    pub(super) fn new(source: Deferred<'input, 'd, R>, policy: hold::Policy) -> Self {
        let logical = source.len();
        let choice = policy.choose(logical, || R::charge(&source.input.0).resident_bytes);
        let Deferred { input } = source;
        Self { input, choice }
    }

    /// Maximum resident bytes produced when this exact bound is retained.
    pub fn ceiling(&self) -> usize {
        match self.choice {
            Choice::Empty => 0,
            Choice::Copy => self.len(),
            Choice::Share => self.len().max(R::charge(&self.input.0).resident_bytes),
        }
    }

    pub fn try_retain(self) -> Result<resident::Shared<'d>, (Self, hold::Error)> {
        let budget = self.input.1;
        self.try_admit(budget)
    }

    pub fn try_admit(
        self,
        budget: &resident::Budget<'d>,
    ) -> Result<resident::Shared<'d>, (Self, hold::Error)> {
        self.choice
            .try_retain::<R>(&self.input.0, budget)
            .map_err(|error| (self, error))
    }

    /// Retains this range and ends the bound if resident admission fails.
    pub fn try_into(self) -> Result<resident::Shared<'d>, hold::Error> {
        self.choice.try_retain::<R>(&self.input.0, self.input.1)
    }
}

const _: () = {
    let word = size_of::<usize>();
    assert!(size_of::<Bound<'static, 'static, storage::Shared>>() == 5 * word);
    assert!(size_of::<Option<Bound<'static, 'static, storage::Shared>>>() == 5 * word);
    assert!(size_of::<Result<resident::Shared<'static>, hold::Error>>() == 5 * word);
    assert!(
        size_of::<
            Result<
                resident::Shared<'static>,
                (Bound<'static, 'static, storage::Shared>, hold::Error),
            >,
        >() == 9 * word
    );
};

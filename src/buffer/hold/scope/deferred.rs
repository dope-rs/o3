use buffer::Read as _;
use hold::scope::Bound;

use crate::{
    buffer,
    buffer::{hold, resident, write::consume},
};

#[must_use]
pub struct Deferred<'input, 'd, R: buffer::Source + 'input> {
    pub(in crate::buffer::hold) input: (R::View<'input>, &'input resident::Budget<'d>),
}

impl<'input, 'd, R: buffer::Source + 'input> Deferred<'input, 'd, R> {
    pub fn as_slice(&self) -> &'input [u8] {
        self.input.0.as_slice()
    }

    pub const fn budget(&self) -> &'input resident::Budget<'d> {
        self.input.1
    }

    pub fn len(&self) -> usize {
        self.as_slice().len()
    }

    pub fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }

    pub fn bind(self, policy: hold::Policy) -> Bound<'input, 'd, R>
    where
        R: hold::raw::Retain<'d>,
    {
        Bound::new(self, policy)
    }

    pub fn try_split<const N: usize>(self) -> Result<(&'input [u8; N], Self), consume::Error> {
        let (head, plan) = self.input.0.try_split()?;
        Ok((
            head,
            Self {
                input: (plan, self.input.1),
            },
        ))
    }
}

pub use cause::Cause;
pub use error::Error;

use std::{convert::Infallible, ops::ControlFlow};

use crate::buffer::resident;

mod cause;
mod error;

pub(in crate::buffer) trait Admission {
    type Declined;

    fn fresh(&self) -> ControlFlow<Self::Declined> {
        ControlFlow::Continue(())
    }

    fn check(&self, _: resident::Budget<'_>, _: usize) -> ControlFlow<Self::Declined> {
        ControlFlow::Continue(())
    }
}

impl Admission for () {
    type Declined = Infallible;
}

pub(in crate::buffer) struct Cap<const FRESH: bool>(pub(in crate::buffer) usize);

impl<const FRESH: bool> Admission for Cap<FRESH> {
    type Declined = ();

    fn fresh(&self) -> ControlFlow<()> {
        if FRESH {
            ControlFlow::Continue(())
        } else {
            ControlFlow::Break(())
        }
    }

    fn check(&self, budget: resident::Budget<'_>, additional: usize) -> ControlFlow<()> {
        let used = budget.resident();
        if used > self.0 || additional > self.0 - used {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    }
}

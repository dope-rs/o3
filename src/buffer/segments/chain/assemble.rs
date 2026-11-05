use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Exact, Many, assembly, chain::edit},
    },
};

impl<T: buffer::raw::Segment> Many<T> {
    pub(in crate::buffer::segments::chain) fn try_append(
        self,
        item: segments::Item<T>,
    ) -> Result<Self, crate::mem::Rejected<(Self, segments::Item<T>), assembly::Error>> {
        let mut chain = self;
        let growth = match edit::Growth::try_new(&chain.0, &item) {
            Ok(state) => state,
            Err(error) => {
                return Err(crate::mem::Rejected::new(
                    (chain, item),
                    assembly::Error::Total(error),
                ));
            }
        };
        match growth.assemble(&mut chain.0, item, edit::Back) {
            edit::Assembly::Local => Ok(chain),
            edit::Assembly::Exact {
                item,
                growth,
                cause,
            } => {
                let current = chain.0.count();
                Ok(Exact::try_reserve((chain, item, growth, cause), current)?.collect())
            }
        }
    }

    pub(in crate::buffer::segments::chain) fn try_prepend(
        self,
        item: segments::Item<T>,
    ) -> Result<Self, crate::mem::Rejected<(segments::Item<T>, Self), assembly::Error>> {
        let mut chain = self;
        let growth = match edit::Growth::try_new(&chain.0, &item) {
            Ok(state) => state,
            Err(error) => {
                return Err(crate::mem::Rejected::new(
                    (item, chain),
                    assembly::Error::Total(error),
                ));
            }
        };
        match growth.assemble(&mut chain.0, item, edit::Front) {
            edit::Assembly::Local => Ok(chain),
            edit::Assembly::Exact {
                item,
                growth,
                cause,
            } => {
                let current = chain.0.count();
                Ok(
                    Exact::try_reserve(edit::Prepend::new((item, chain), growth, cause), current)?
                        .collect(),
                )
            }
        }
    }
}

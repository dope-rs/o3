use crate::{
    buffer,
    buffer::{
        segments,
        segments::{Chain, Exact, assembly, chain::edit},
    },
};

pub(in crate::buffer::segments) trait Assemble<T: buffer::raw::Segment>:
    buffer::raw::Assemble<T> + Into<Chain<T>>
{
    fn sequence(&mut self) -> &mut segments::Chain<T, segments::Pool<T>>;

    fn try_append(
        self,
        item: segments::Item<T>,
    ) -> Result<Self, crate::mem::Rejected<(Self, segments::Item<T>), assembly::Error>> {
        let mut chain = self;
        let growth = match edit::Growth::try_new(chain.sequence(), &item) {
            Ok(state) => state,
            Err(error) => {
                return Err(crate::mem::Rejected::new(
                    (chain, item),
                    assembly::Error::Total(error),
                ));
            }
        };
        match growth.assemble(chain.sequence(), item, edit::Back) {
            edit::Assembly::Local => Ok(chain),
            edit::Assembly::Exact {
                item,
                growth,
                cause,
            } => {
                let current = chain.sequence().count();
                Ok(Exact::try_reserve((chain, item, growth, cause), current)?.collect())
            }
        }
    }

    fn try_prepend(
        self,
        item: segments::Item<T>,
    ) -> Result<Self, crate::mem::Rejected<(segments::Item<T>, Self), assembly::Error>> {
        let mut chain = self;
        let growth = match edit::Growth::try_new(chain.sequence(), &item) {
            Ok(state) => state,
            Err(error) => {
                return Err(crate::mem::Rejected::new(
                    (item, chain),
                    assembly::Error::Total(error),
                ));
            }
        };
        match growth.assemble(chain.sequence(), item, edit::Front) {
            edit::Assembly::Local => Ok(chain),
            edit::Assembly::Exact {
                item,
                growth,
                cause,
            } => {
                let current = chain.sequence().count();
                Ok(
                    Exact::try_reserve(edit::Prepend::new((item, chain), growth, cause), current)?
                        .collect(),
                )
            }
        }
    }
}

use std::num;

use o3::{
    base::cell::region,
    buffer::{self, bytes::scope, storage::provided::Mutable},
    depot::{
        map::compact::{self, Planes},
        queue::{indexed::lanes, resident::fifo, tagged},
    },
    mem::credit::fair,
};

const _: fn(&lanes::Fifo<(), 1>) -> &lanes::Fifo<(), 1> =
    |queue| tagged::Endpoint::constant::<0>(queue).into_store();

fn fifo_lane<'scope>(
    queue: &fifo::Fifo<(), fifo::Scope<'scope>>,
    index: fifo::Index<'scope>,
) -> Result<(), num::TryFromIntError> {
    let compact = fifo::Compact::try_from(index)?;
    let _ = queue.lane(compact.into_index());
    Ok(())
}

fn provided<T: Mutable<'static>>(value: T) -> impl buffer::raw::Split {
    value
}

fn main() -> Result<(), compact::Error> {
    scope::Owner::with((), |owner| owner.into_inner());

    let _: fair::Cause<usize, 1> = fair::Cause::Overflow {
        axis: fair::Axis::<1>::ALL[0],
        held: 0,
        requested: 0,
    };

    let (planes, _) = Planes::try_new([1], 0)?;
    let _ = planes.layout::<0>();

    region::Token::scope(|token| {
        let value = token.bind(0);
        let _ = token.borrow(&value);
    });
    Ok(())
}

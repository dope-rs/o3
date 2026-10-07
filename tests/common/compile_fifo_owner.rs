use std::num::NonZero;

use o3::depot::queue::resident::{self, fifo};

fn main() -> Result<(), resident::Error> {
    let lanes = NonZero::<usize>::MIN.saturating_add(1);
    fifo::Fifo::<()>::try_with(0, lanes, (), |_, first| {
        let Ok(index) = first.try_index(1) else {
            return;
        };
        let Ok(index) = fifo::Compact::try_from(index) else {
            return;
        };
        let _ = fifo::Fifo::<()>::try_with(0, lanes, (), |_, second| {
            let _ = second.lane(index.into_index());
        });
    })
}

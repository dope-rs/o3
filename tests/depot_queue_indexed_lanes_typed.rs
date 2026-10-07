pub mod common;

use std::{cell::Cell, convert::Infallible};

use common::{Counted, Owner, alloc::system::Tracker, depot::index::key::Id};
use o3::depot::{
    array::Index,
    capacity,
    queue::indexed::lanes::{self, typed, typed::Vacancy},
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

const _: () = {
    assert!(
        typed::Table::<Id<'static>, u64, u64, u64, 2>::NODE_BYTES
            <= lanes::Table::<Id<'static>, typed::Value<u64, u64, u64>, 2>::NODE_BYTES
    );
    assert!(
        typed::Table::<
            Id<'static>,
            Owner<'static>,
            (u8, Owner<'static>),
            (Owner<'static>, [u64; 2]),
            2,
        >::NODE_BYTES
            <= lanes::Table::<
                Id<'static>,
                typed::Value<Owner<'static>, (u8, Owner<'static>), (Owner<'static>, [u64; 2])>,
                2,
            >::NODE_BYTES
    );
    assert!(typed::Table::<Id<'static>, [u64; 23], (), Infallible, 7>::NODE_BYTES == 208);
};

#[test]
fn retains_phase_and_fifo() {
    let controls = Cell::new(Some(0));
    let data = Cell::new(Some(0));
    let idle = Cell::new(Some(0));
    let keys = [0, 1, 2, 3].map(|index| Id::try_new(index, 1).expect("bounded identity"));
    let ready = Index::at::<0>();
    let waiting = Index::at::<1>();
    let mut table =
        typed::Table::<Id<'_>, Owner<'_>, (u8, Owner<'_>), (Owner<'_>, [u64; 2]), 2>::try_new(
            capacity::Count::new(4),
        )
        .expect("four slots allocate");
    table
        .vacant(keys[0])
        .expect("empty slot")
        .insert::<typed::Control>(Owner {
            drops: Counted(&controls),
            value: 7,
        })
        .move_to_back(ready);
    table
        .vacant(keys[1])
        .expect("empty slot")
        .insert::<typed::Data>((
            9,
            Owner {
                drops: Counted(&data),
                value: 11,
            },
        ))
        .move_to_back(ready);
    let _ = table
        .vacant(keys[2])
        .expect("empty slot")
        .insert::<typed::Idle>((
            Owner {
                drops: Counted(&idle),
                value: 17,
            },
            [19, 23],
        ));
    let _ = table
        .vacant(keys[3])
        .expect("empty slot")
        .insert::<typed::Data>((
            13,
            Owner {
                drops: Counted(&data),
                value: 29,
            },
        ));

    let ((), counts) = Tracker::measure(|| {
        let address = match table.entry(keys[1]).expect("resident data") {
            typed::Entry::Data(mut slot) => {
                slot.get_mut().1.value += 1;
                Some(slot.get() as *const _)
            }
            _ => None,
        }
        .expect("data phase");
        assert_eq!(data.get(), Some(0));
        assert_eq!(table.back(ready), Some(keys[1]));

        let slot = match table.front(ready) {
            Some(typed::Front::Control(slot)) => Some(slot),
            _ => None,
        }
        .expect("control front");
        let (vacant, value) = slot.take();
        assert_eq!(value.value, 7);
        let _ = vacant.restore(value);
        let slot = match table.front(ready) {
            Some(typed::Front::Control(slot)) => Some(slot),
            _ => None,
        }
        .expect("restored control front");
        assert_eq!(slot.key(), keys[0]);
        let (vacant, value) = slot.take();
        drop(value);
        vacant
            .insert::<typed::Data>((
                31,
                Owner {
                    drops: Counted(&data),
                    value: 37,
                },
            ))
            .move_to_back(waiting);
        assert_eq!(controls.get(), Some(1));

        let mut slot = match table.front(ready) {
            Some(typed::Front::Data(slot)) => Some(slot),
            _ => None,
        }
        .expect("data front");
        assert_eq!(slot.get() as *const _, address);
        assert_eq!(slot.get().0, 9);
        assert_eq!(slot.get_mut().1.value, 12);
        slot.unlink();
        let slot = match table.entry(keys[1]) {
            Some(typed::Entry::Data(slot)) => Some(slot),
            _ => None,
        }
        .expect("unlinked data");
        assert_eq!(slot.lane(), None);
        let (vacant, value) = slot.take();
        drop(value);
        let _ = vacant.insert::<typed::Idle>((
            Owner {
                drops: Counted(&idle),
                value: 41,
            },
            [43, 47],
        ));
        assert_eq!(data.get(), Some(1));

        let slot = match table.entry(keys[2]) {
            Some(typed::Entry::Idle(slot)) => Some(slot),
            _ => None,
        }
        .expect("resident idle");
        let (vacant, value) = slot.take();
        assert_eq!(value.1, [19, 23]);
        let slot = vacant.restore(value);
        assert_eq!(slot.lane(), None);
        drop(slot.remove());
        assert_eq!(idle.get(), Some(1));

        let slot = match table.entry(keys[3]) {
            Some(typed::Entry::Data(slot)) => Some(slot),
            _ => None,
        }
        .expect("initially unlinked data");
        assert_eq!(slot.lane(), None);
        slot.move_to_back(waiting);
        assert_eq!(table.move_all(waiting, ready), 2);
        assert_eq!(table.back(ready), Some(keys[3]));
        let slot = match table.front(ready) {
            Some(typed::Front::Data(slot)) => Some(slot),
            _ => None,
        }
        .expect("successor front");
        assert_eq!(slot.key(), keys[0]);
        assert_eq!(slot.get().0, 31);
        let (vacant, value) = slot.take();
        drop(vacant);
        drop(value);
        assert_eq!(table.len(), 2);
        assert_eq!(data.get(), Some(2));
        table.clear();
        assert!(table.is_empty());
        assert!(table.is_vacant(ready));
        assert_eq!(data.get(), Some(3));
        assert_eq!(idle.get(), Some(2));
    });
    assert_eq!(counts, [0; 4]);
    table
        .vacant(keys[0])
        .expect("cleared slot")
        .insert::<typed::Control>(Owner {
            drops: Counted(&controls),
            value: 53,
        })
        .move_to_back(ready);
    drop(table);
    assert_eq!(controls.get(), Some(2));
}

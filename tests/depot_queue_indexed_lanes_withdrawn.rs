pub mod common;

use std::{cell::Cell, mem};

use common::{Counted, Owner, alloc::system::Tracker, depot::index::key::Id};
use o3::depot::{array::Index, capacity, queue::indexed::lanes};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn preserves_fifo_owners() {
    const {
        assert!(!mem::needs_drop::<
            lanes::entry::Withdrawn<'_, Id<'_>, Owner<'_>, 2>,
        >());
    };

    let drops = Cell::new(Some(0));
    let keys = [0, 1, 2, 3].map(|index| Id::try_new(index, 1).expect("identity components fit"));
    let ready = Index::at::<0>();
    let waiting = Index::at::<1>();
    let mut table = lanes::Table::<Id<'_>, Owner<'_>, 2>::try_new(capacity::Count::new(4))
        .expect("four lane slots allocate");
    for (index, key) in keys.into_iter().enumerate() {
        let slot = table
            .vacant(key)
            .expect("distinct vacant slot")
            .insert(Owner {
                drops: Counted(&drops),
                value: index,
            });
        if index < 3 {
            slot.move_to_back(ready);
        }
    }

    let ((), counts) = Tracker::measure(|| {
        for key in keys {
            let (vacant, value) = table.entry(key).expect("resident owner").take();
            let _ = vacant.restore(value);
            assert!(table.front(ready).map(|slot| slot.key()) == Some(keys[0]));
            assert!(table.back(ready) == Some(keys[2]));
            assert_eq!(table.len(), 4);
        }
        for key in &keys[..3] {
            let front = table.front(ready).expect("restored FIFO member");
            assert!(front.key() == *key);
            front.move_to_back(ready);
        }
        assert!(table.lane(keys[3]) == Some(None));
        assert_eq!(drops.get(), Some(0));

        let value = table.entry(keys[1]).expect("middle owner").take().1;
        assert_eq!(value.value, 1);
        drop(value);
        assert!(table.get(keys[1]).is_none());
        assert_eq!(table.len(), 3);

        let (vacant, value) = table.front(ready).expect("first owner").take();
        vacant.insert(value).move_to_back(waiting);
        assert!(table.front(ready).map(|slot| slot.key()) == Some(keys[2]));
        assert!(table.back(ready) == Some(keys[2]));
        assert!(table.front(waiting).map(|slot| slot.key()) == Some(keys[0]));

        let (vacant, value) = table.front(ready).expect("only owner").take();
        let _ = vacant.restore(value);
        assert!(table.front(ready).map(|slot| slot.key()) == Some(keys[2]));
        assert!(table.back(ready) == Some(keys[2]));

        let (vacant, value) = table.entry(keys[3]).expect("unlinked owner").take();
        let _ = vacant.insert(value);
        assert!(table.lane(keys[3]) == Some(None));

        let (_, value) = table.front(ready).expect("last ready owner").take();
        drop(value);
        assert!(table.is_vacant(ready));
        assert_eq!(table.len(), 2);
        assert_eq!(drops.get(), Some(2));
    });
    assert_eq!(counts, [0; 4]);
    drop(table);
    assert_eq!(drops.get(), Some(4));
}

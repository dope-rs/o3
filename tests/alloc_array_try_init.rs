pub mod common;

use std::{cell::Cell, mem};

use common::alloc::{Owner, system::Tracker};
use o3::alloc::array;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn retains_final_addresses() {
    let bytes = 3 * mem::size_of::<usize>();
    let ((), activity) = Tracker::measure(|| {
        let block =
            array::Block::<mem::MaybeUninit<usize>, _>::try_new(3usize, ()).expect("allocation");
        let block = block
            .try_init(|slot| Ok::<_, ()>(slot.as_ptr().addr()))
            .expect("initialization");
        for value in &*block {
            assert_eq!(*value, std::ptr::from_ref(value).addr());
        }
    });
    assert_eq!(activity, [1, bytes, 1, bytes]);
}

#[test]
fn failure_drops_prefix() {
    let drops = Cell::new(Some(0));
    let mut digit = 0;
    let bytes = 4 * mem::size_of::<Owner<'_>>();
    let (result, activity) = Tracker::measure(|| {
        let drops = &drops;
        let factory = Owner { drops, digit: 8 };
        let block = array::Block::try_new(4usize, Owner { drops, digit: 9 }).expect("allocation");
        block.try_init(move |_| {
            let _ = &factory;
            digit += 1;
            if digit == 3 {
                Err(17u8)
            } else {
                Ok(Owner { drops, digit })
            }
        })
    });
    assert!(matches!(result, Err(17)));
    assert_eq!(drops.get(), Some(1289));
    assert_eq!(activity, [1, bytes, 1, bytes]);
}

#[test]
fn status_drops_value() {
    let drops = Cell::new(Some(0));
    let bytes = 4 * mem::size_of::<Owner<'_>>();
    let (result, activity) = Tracker::measure(|| {
        let drops = &drops;
        let factory = Owner { drops, digit: 8 };
        let block = array::Block::try_new(4usize, Owner { drops, digit: 9 }).expect("allocation");
        let mut digit = 0;
        block.try_fill(move |slot| {
            let _ = &factory;
            digit += 1;
            let done = slot.insert(Owner { drops, digit });
            if digit == 3 { Err(17u8) } else { Ok(done) }
        })
    });
    assert!(matches!(result, Err(17)));
    assert_eq!(drops.get(), Some(12389));
    assert_eq!(activity, [1, bytes, 1, bytes]);
}

#[test]
fn clones_slice() {
    for (source, expected, count) in [(&[][..], 0, 0), (&[3u8, 1, 2][..], 312, 1)] {
        let mut seen = 0;
        let ((), activity) = Tracker::measure(|| {
            let block = array::Block::try_clone(source, |value| {
                seen = seen * 10 + usize::from(*value);
                Ok(*value)
            })
            .expect("clone");
            assert_eq!(&*block, source);
        });
        assert_eq!(seen, expected);
        assert_eq!(activity, [count, source.len(), count, source.len()]);
    }
}

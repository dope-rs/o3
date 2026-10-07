pub mod common;

use std::{cell::Cell, mem, num::NonZero};

use common::{
    alloc::system::Tracker,
    depot::{
        index::{Bitmap, Borrowed},
        model::Lcg,
    },
};
use o3::depot::{
    capacity::{Count, Limit},
    index::bitmap::{self, entry::Row},
    raw::Proof,
};

// SAFETY: construction lends an exact zeroed plane within its declared depth;
// only bitmap row operations mutate the borrowed words.

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn borrows_exact_planes() {
    for capacity in [0, 1, 64, 65, 4096, 4097, 262145] {
        let shape = bitmap::Shape::<u32>::new(capacity);
        let cells = vec![Cell::new(0); shape.words()];
        let source = unsafe { Borrowed::new(shape, &cells) };
        let ((), counts) = Tracker::measure(|| {
            for index in 0..capacity as usize {
                let row = Row::try_new(&source, index).expect("bounded row");
                assert!(!row.contains(()));
                row.update::<true>(());
                assert!(row.contains(()));
            }
            for index in 0..capacity as usize {
                let row = bitmap::Access::find(&source, (), 0).expect("remaining row");
                assert_eq!(row.index(), index);
                row.update::<false>(());
            }
            assert!(bitmap::Access::find(&source, (), 0).is_none());
            assert!(bitmap::Access::is_empty(&source, ()));
        });
        assert_eq!(counts, [0; 4]);
        assert!(cells.iter().all(|word| word.get() == 0));
    }
}

#[test]
fn sizes_boundary_levels() {
    let radix = usize::BITS;
    for (bits, words) in [
        (0, 0),
        (1, 1),
        (radix, 1),
        (radix + 1, 3),
        (radix * radix, radix + 1),
        (radix * radix + 1, radix + 4),
    ] {
        let shape = bitmap::Shape::<u32>::new(bits);
        assert_eq!(shape.bits(), bits as usize);
        assert_eq!(shape.words(), words as usize);
        if let Some(bits) = NonZero::new(bits) {
            assert_eq!(
                bitmap::Shape::<NonZero<u32>>::new(bits).words(),
                shape.words()
            );
        }
    }
    let shape = bitmap::Shape::<u32>::new(u32::MAX);
    let mut bits = u64::from(u32::MAX);
    let mut words = 0;
    while bits != 1 {
        bits = bits.div_ceil(u64::from(radix));
        words += bits;
    }
    assert_eq!(shape.words() as u64, words);
}

#[test]
fn rejects_first_admission() {
    for capacity in [0, 1, 64, 65, 4097] {
        let mut owner = Bitmap::try_new(bitmap::Shape::<u32>::new(capacity), bitmap::Fill::Empty)
            .expect("bitmap storage allocates");
        for index in [capacity as usize, usize::MAX] {
            let error = Row::try_new(&owner, index).err().expect("invalid row");
            assert_eq!(
                error,
                bitmap::Error {
                    index,
                    capacity: capacity as usize
                }
            );
        }
        if capacity != 0 {
            let row = Row::try_new(&mut owner, capacity as usize - 1).expect("bounded row");
            let absent = row.occupied(()).err().expect("empty row");
            assert_eq!(absent.index(), capacity as usize - 1);
        }
    }
}

#[test]
fn consumes_occupancy() {
    let mut owner = Bitmap::try_new(bitmap::Shape::<u32>::new(4097), bitmap::Fill::Empty)
        .expect("bitmap storage allocates");
    let row = Row::try_new(&owner, 4096).expect("row");
    row.update::<true>(());
    row.update::<true>(());
    let ((), counts) = Tracker::measure(|| {
        let row = Row::try_new(&mut owner, 4096).expect("row");
        let result = row.occupied(());
        let occupied = result
            .map_err(|error| error.index())
            .expect("bitmap index must be occupied");

        assert_eq!(occupied.index(), 4096);
        let removed = occupied.remove();
        assert_eq!(removed.index(), 4096);
    });
    assert_eq!(counts, [0; 4]);
    assert!(bitmap::Access::is_empty(&owner, ()));
    let row = Row::try_new(&owner, 4096).expect("row");
    row.update::<false>(());
    row.update::<false>(());
    assert!(bitmap::Access::find(&owner, (), 0).is_none());
}

#[test]
fn traverses_summaries() {
    for capacity in [0, 1, 63, 64, 65, 4095, 4096, 4097, 262145] {
        let mut owner = Bitmap::try_new(bitmap::Shape::<u32>::new(capacity), bitmap::Fill::Full)
            .expect("bitmap storage allocates");
        let ((), counts) = Tracker::measure(|| {
            for index in 0..capacity {
                let member = bitmap::Access::first(&mut owner, ()).expect("remaining member");
                assert_eq!(member.index(), index as usize);
                assert_eq!(member.remove().index(), index as usize);
            }
            assert!(bitmap::Access::is_empty(&owner, ()));
            assert!(bitmap::Access::pop_from(&owner, (), usize::MAX).is_none());
        });
        assert_eq!(counts, [0; 4]);
    }
}

#[test]
fn retains_nonempty_domain() {
    for capacity in [
        NonZero::<u32>::MIN,
        NonZero::new(65).expect("positive"),
        NonZero::new(4097).expect("positive"),
    ] {
        let owner = Bitmap::try_new(
            bitmap::Shape::<NonZero<u32>>::new(capacity),
            bitmap::Fill::Empty,
        )
        .expect("bitmap storage allocates");
        let ((), counts) = Tracker::measure(|| {
            let entry = Row::clamp(&owner, usize::MAX);
            assert_eq!(entry.index(), capacity.get() as usize - 1);
            let position = entry.position();
            entry.update::<true>(());
            let removed = bitmap::Access::pop_from(&owner, (), 0).expect("last row");
            assert_eq!(removed.index(), entry.index());
            // SAFETY: the retained position came from this owner's unchanged domain;
            // membership changed, but the physical row remained initialized.
            let source = unsafe { Proof::new((&owner, position)) };
            let entry = Row::from_source(source);
            assert!(!entry.contains(()));
            entry.update::<true>(());
            entry.update::<false>(());
        });
        assert_eq!(counts, [0; 4]);
    }
}

#[test]
fn preserves_layout() {
    assert_eq!(size_of::<Row<&Bitmap>>(), 2 * size_of::<usize>());
    assert_eq!(size_of::<Option<Row<&Bitmap>>>(), 2 * size_of::<usize>());
    assert_eq!(
        size_of::<Row<&mut Bitmap, (), bitmap::entry::Occupied>>(),
        2 * size_of::<usize>()
    );
    assert_eq!(
        size_of::<Option<Row<&mut Bitmap, (), bitmap::entry::Occupied>>>(),
        2 * size_of::<usize>()
    );
    assert_eq!(
        size_of::<Row<&Bitmap, (), bitmap::entry::Removed>>(),
        2 * size_of::<usize>()
    );
}

#[test]
fn preserves_summaries() {
    for capacity in [1, 63, 64, 65, 4095, 4096, 4097, 262145] {
        let owner = Bitmap::try_new(bitmap::Shape::<u32>::new(capacity), bitmap::Fill::Empty)
            .expect("bitmap storage allocates");
        let mut model = vec![false; capacity as usize];
        let mut rng = Lcg::<1>::new(0xc37a_0b15_91d4_ee6du64);
        let ((), counts) = Tracker::measure(|| {
            for step in 0..4096 {
                let seed = rng.advance();
                let index = (seed >> 32) as usize % model.len();
                let row = Row::try_new(&owner, index).expect("bounded row");
                if step % 3 == 0 {
                    row.update::<false>(());
                    model[index] = false;
                } else {
                    row.update::<true>(());
                    model[index] = true;
                }
                assert_eq!(row.contains(()), model[index]);
                let start = if step % 7 == 0 {
                    usize::MAX
                } else {
                    seed as usize % model.len()
                };
                let expected = model.iter().enumerate().skip(start).find(|(_, set)| **set);
                assert_eq!(
                    bitmap::Access::find(&owner, (), start).map(|row| row.index()),
                    expected.map(|(index, _)| index)
                );
                if step % 4 == 0 {
                    let offset = start % model.len();
                    let expected = (offset..model.len())
                        .chain(0..offset)
                        .find(|index| model[*index]);
                    let removed = bitmap::Access::pop_from(&owner, (), start);
                    assert_eq!(removed.as_ref().map(|row| row.index()), expected);
                    if let Some(index) = expected {
                        model[index] = false;
                    }
                }
                assert_eq!(
                    bitmap::Access::is_empty(&owner, ()),
                    !model.iter().any(|set| *set)
                );
            }
        });
        assert_eq!(counts, [0; 4]);
    }
}

#[test]
fn preserves_count_domain() {
    use o3::alloc::{array::Block, raw::Empty, tail};

    assert!(Limit::<65536>::new(0).is_none());
    assert!(Limit::<65536>::new(65537).is_none());
    assert_eq!(Count::EMPTY.raw(), 0);
    assert_eq!(Count::MAX.raw(), u32::MAX);
    assert_eq!(Count::new(17).raw(), 17);
    let max = Count::try_from(u32::MAX as usize).expect("the u32 maximum must fit capacity");
    assert!(max == Count::MAX);
    for raw in [1, u32::MAX] {
        let count = NonZero::new(raw as usize).expect("nonzero");
        assert_eq!(Count::try_nonzero(count).expect("representable").get(), raw);
    }
    if usize::BITS > 32 {
        for needed in [u32::MAX as usize + 1, usize::MAX] {
            let error = Count::try_from(needed).expect_err("capacity above u32");
            assert_eq!(error.requested(), needed);
            let count = NonZero::new(needed).expect("nonzero");
            assert_eq!(
                Count::try_nonzero(count)
                    .expect_err("count overflow")
                    .requested(),
                needed
            );
        }
    }
    assert_eq!(
        mem::size_of::<tail::Plan<u32, u32, Count>>(),
        mem::size_of::<Count>()
    );
    for len in [0, 3, 17] {
        let plan = (tail::Plan::<u32, u32, Count>::try_new(Count::new(len)))
            .expect("the typed count must fit its tail layout");
        let memory = (plan.try_allocate()).expect("the typed tail plan must allocate");
        let block = memory.fill(5, |header, index| header + index);
        assert_eq!(tail::View::new(&block).header(), &5);
        assert!(
            tail::View::new(&block)
                .as_slice()
                .iter()
                .copied()
                .eq(5..5 + len)
        );
    }
    let ((), counts) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            const EMPTY: Block<u64, Count> = Block::empty(Empty::EMPTY, ());
            assert!(EMPTY.is_empty());
        });
    });
    assert_eq!(counts, [0; 4]);
}

pub mod common;

use std::{cell, mem};

use common::alloc::{
    AlignedZst, Owner,
    owner::{self, Aligned},
    system::Tracker,
};
use o3::alloc::{self, array};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn defaults_preserve_owners() {
    let (built, cold) = Tracker::measure(|| {
        let values: Result<Box<[Box<u16>]>, alloc::Error> = alloc::BoxSliceExt::try_default(3);
        values
    });
    let bytes = 3 * (mem::size_of::<Box<u16>>() + mem::size_of::<u16>());
    assert_eq!(cold, [4, bytes, 0, 0]);
    let mut values = built.expect("outer slice and three default owners");
    *values[0] = 7;
    let ((), freed) = Tracker::measure(move || {
        assert_eq!((*values[0], *values[1], *values[2]), (7, 0, 0));
    });
    assert_eq!(freed, [0, 0, 4, bytes]);

    let (failed, activity) = Tracker::measure(|| {
        let values: Result<Box<[Box<u16>]>, alloc::Error> =
            Tracker::fail_after(0, || alloc::BoxSliceExt::try_default(3));
        values
    });
    assert!(matches!(failed, Err(alloc::Error::Exhausted)));
    assert_eq!(activity, [0; 4]);
}

#[test]
fn slice_keeps_borrows() {
    let source = [String::from("left"), String::from("right")];
    let borrowed: Result<Box<[&str]>, _> =
        alloc::BoxSliceExt::try_from_fn(source.len(), |index| source[index].as_str());
    let borrowed = (borrowed).expect("the two-element borrowed slice must allocate");
    assert_eq!(&*borrowed, ["left", "right"]);
}

#[test]
fn aligned_layouts_match() {
    let (built, cold) = Tracker::measure(|| {
        let boxed: Result<Box<Aligned>, alloc::Exhausted> = alloc::BoxExt::try_new(Aligned(7));
        let slice: Result<Box<[Aligned]>, alloc::Error> =
            alloc::BoxSliceExt::try_from_fn(3, |index| Aligned(index as u8));
        (boxed, slice)
    });
    let bytes = 4 * mem::size_of::<Aligned>();
    assert_eq!(cold, [2, bytes, 0, 0]);
    let boxed = built.0.expect("aligned box allocation");
    let slice = built.1.expect("aligned slice allocation");
    assert_eq!(boxed.0, 7);
    assert_eq!(
        slice.iter().map(|entry| entry.0).collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!(
        boxed.as_ref() as *const Aligned as usize % mem::align_of::<Aligned>(),
        0
    );
    assert_eq!(slice.as_ptr() as usize % mem::align_of::<Aligned>(), 0);

    let ((), freed) = Tracker::measure(|| drop((boxed, slice)));
    assert_eq!(freed, [0, 0, 2, bytes]);
}

#[test]
fn zsts_allocate_nothing() {
    let (built, activity) = Tracker::measure(|| {
        let boxed: Result<Box<AlignedZst>, alloc::Exhausted> = alloc::BoxExt::try_new(AlignedZst);
        let slice: Result<Box<[AlignedZst]>, alloc::Error> =
            alloc::BoxSliceExt::try_repeat(8, AlignedZst);
        let empty: Result<Box<[Aligned]>, alloc::Error> =
            alloc::BoxSliceExt::try_repeat(0, Aligned(0));
        let values: Result<Vec<AlignedZst>, alloc::Error> = alloc::VecExt::try_new(usize::MAX);
        (boxed, slice, empty, values)
    });
    assert_eq!(activity, [0; 4]);
    let boxed = built.0.expect("zero-sized box");
    let slice = built.1.expect("zero-sized slice");
    let empty = built.2.expect("empty slice");
    let values = built.3.expect("zero-sized vector");
    assert_eq!(
        boxed.as_ref() as *const AlignedZst as usize % mem::align_of::<AlignedZst>(),
        0
    );
    assert_eq!(slice.as_ptr() as usize % mem::align_of::<AlignedZst>(), 0);
    assert_eq!(empty.as_ptr() as usize % mem::align_of::<Aligned>(), 0);
    assert_eq!(values.as_ptr().addr() % mem::align_of::<AlignedZst>(), 0);
    assert_eq!(values.capacity(), usize::MAX);
    assert_eq!((slice.len(), empty.len(), values.len()), (8, 0, 0));
}

#[test]
fn array_uninit_keeps_rows() {
    let bytes = 3 * mem::size_of::<u32>();
    let (rows, cold) = Tracker::measure(|| {
        let rows: Result<Box<[mem::MaybeUninit<u32>; 3]>, alloc::Exhausted> =
            alloc::BoxArrayExt::try_uninit();
        rows
    });
    assert_eq!(cold, [1, bytes, 0, 0]);
    let mut rows = rows.expect("three uninitialized rows must allocate");
    rows[0].write(7);
    rows[2].write(9);
    // SAFETY: only rows 0 and 2 are read, and both were written above.
    let (first, last) = unsafe { (rows[0].assume_init(), rows[2].assume_init()) };
    assert_eq!((first, last), (7, 9));
    let ((), freed) = Tracker::measure(move || {
        let released = rows;
        assert_eq!(released.len(), 3);
    });
    assert_eq!(freed, [0, 0, 1, bytes]);
}

#[test]
fn array_uninit_failure() {
    let (failed, activity) = Tracker::measure(|| {
        let rows: Result<Box<[mem::MaybeUninit<u8>; 4]>, alloc::Exhausted> =
            Tracker::fail_after(0, alloc::BoxArrayExt::try_uninit);
        rows
    });
    assert!(matches!(failed, Err(alloc::Exhausted)));
    assert_eq!(activity, [0; 4]);
}

#[test]
fn copy_keeps_layout() {
    let source = [Aligned(3), Aligned(7)];
    let (copied, cold) = Tracker::measure(|| {
        let copied: Result<Box<[Aligned]>, alloc::Exhausted> =
            alloc::BoxSliceExt::try_copy(&source);
        copied
    });
    assert_eq!(cold, [1, mem::size_of_val(&source), 0, 0]);
    let copied = (copied).expect("copy must allocate the exact source layout");
    let ((), freed) = Tracker::measure(move || {
        assert_eq!((copied[0].0, copied[1].0), (3, 7));
        assert_eq!(copied.as_ptr() as usize % mem::align_of::<Aligned>(), 0);
        assert_ne!(copied.as_ptr(), source.as_ptr());
    });
    assert_eq!(freed, [0, 0, 1, mem::size_of_val(&source)]);
    assert_eq!(
        mem::size_of::<Result<Box<[Aligned]>, alloc::Exhausted>>(),
        mem::size_of::<Box<[Aligned]>>()
    );
}

#[test]
fn copy_keeps_borrows() {
    let source = [String::from("left"), String::from("right")];
    let values = [source[0].as_str(), source[1].as_str()];
    let copied: Result<Box<[&str]>, alloc::Exhausted> = alloc::BoxSliceExt::try_copy(&values);
    let copied = (copied).expect("copy must retain the borrowed elements");
    assert_eq!(&*copied, values);
    assert_eq!(copied[0].as_ptr(), source[0].as_ptr());
    assert_eq!(copied[1].as_ptr(), source[1].as_ptr());
}

#[test]
fn copy_allocates_no_zsts() {
    let zsts = [AlignedZst; usize::MAX];
    let (copied, activity) = Tracker::measure(|| {
        let zsts: Result<Box<[AlignedZst]>, alloc::Exhausted> = alloc::BoxSliceExt::try_copy(&zsts);
        let empty: Result<Box<[Aligned]>, alloc::Exhausted> = alloc::BoxSliceExt::try_copy(&[]);
        (zsts, empty)
    });
    assert_eq!(activity, [0; 4]);
    let zsts = copied.0.expect("zero-sized copy");
    let empty = copied.1.expect("empty copy");
    assert_eq!((zsts.len(), empty.len()), (usize::MAX, 0));
    assert_eq!(zsts.as_ptr() as usize % mem::align_of::<AlignedZst>(), 0);
    assert_eq!(empty.as_ptr() as usize % mem::align_of::<Aligned>(), 0);
}

#[test]
fn copy_failure_is_inert() {
    let source = [3u64, 7];
    let (copied, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            let copied: Result<Box<[u64]>, alloc::Exhausted> =
                alloc::BoxSliceExt::try_copy(&source);
            copied
        })
    });
    assert!(matches!(copied, Err(alloc::Exhausted)));
    assert_eq!(activity, [0; 4]);
    assert_eq!(source, [3, 7]);
}

#[test]
fn init_failure_is_inert() {
    let mut calls = 0;
    let mut init = || {
        calls += 1;
        7_u16
    };
    let (results, activity) = Tracker::measure(|| {
        let overflow: Result<Box<[u16]>, alloc::Error> =
            alloc::BoxSliceExt::try_init(usize::MAX, &mut init);
        let failed: Result<Box<[u16]>, alloc::Error> =
            Tracker::fail_after(0, || alloc::BoxSliceExt::try_init(2, &mut init));
        (overflow, failed)
    });
    assert!(matches!(results.0, Err(alloc::Error::Overflow)));
    assert!(matches!(results.1, Err(alloc::Error::Exhausted)));
    assert_eq!(calls, 0);
    assert_eq!(activity, [0; 4]);
}

#[test]
fn init_releases_owners() {
    let drops = cell::Cell::new(Some(0));
    let mut calls = 0;
    let ((), activity) = Tracker::measure(|| {
        let values: Result<Box<[owner::Owner<'_>]>, _> =
            alloc::BoxSliceExt::try_from_fn(4, |index| {
                calls += 1;
                owner::Owner {
                    drops: &drops,
                    value: Box::new(index + 1),
                }
            });
        let values = values.expect("four initialized owners");
        assert_eq!(values.len(), 4);
    });
    assert_eq!(calls, 4);
    assert_eq!(drops.get(), Some(1234));
    let bytes = 4 * (mem::size_of::<owner::Owner<'_>>() + mem::size_of::<usize>());
    assert_eq!(activity, [5, bytes, 5, bytes]);
}

#[test]
fn init_counts_zsts() {
    let mut calls = 0;
    let (built, activity) = Tracker::measure(|| {
        let mut init = |index| {
            assert_eq!(index, calls);
            calls += 1;
            AlignedZst
        };
        let empty: Result<Box<[AlignedZst]>, alloc::Error> =
            alloc::BoxSliceExt::try_from_fn(0, &mut init);
        let values: Result<Box<[AlignedZst]>, alloc::Error> =
            alloc::BoxSliceExt::try_from_fn(8, init);
        (empty, values)
    });
    assert_eq!(calls, 8);
    assert_eq!(activity, [0; 4]);
    let empty = built.0.expect("empty initialization");
    let values = built.1.expect("zero-sized initialization");
    assert_eq!((empty.len(), values.len()), (0, 8));
    assert_eq!(values.as_ptr() as usize % mem::align_of::<AlignedZst>(), 0);
}

#[test]
fn block_owns_elements() {
    let drops = cell::Cell::new(Some(0));
    let (block, activity) = Tracker::measure(|| {
        array::Block::try_new(
            3_usize,
            Owner {
                drops: &drops,
                digit: 4,
            },
        )
        .map(|block| {
            block.init(|index| Owner {
                drops: &drops,
                digit: index + 1,
            })
        })
    });
    let mut block = block.expect("initialized element allocation");
    let bytes = 3 * mem::size_of::<Owner<'_>>();
    assert_eq!(activity, [1, bytes, 0, 0]);
    assert_eq!(*block.extent(), 3);
    assert_eq!(block.state().digit, 4);
    block[1].digit = 5;
    let ((), released) = Tracker::measure(move || {
        assert_eq!(block.iter().map(|value| value.digit).sum::<usize>(), 9);
    });
    assert_eq!(drops.get(), Some(1534));
    assert_eq!(released, [0, 0, 1, bytes]);
}

#[test]
fn block_assume_init() {
    let drops = cell::Cell::new(Some(0));
    let mut block = array::Block::try_new(
        3_usize,
        Owner {
            drops: &drops,
            digit: 9,
        },
    )
    .expect("allocation before ownership transfer");
    let pointer = block.as_ptr().cast::<Owner<'_>>();
    let (state, slots) = block.parts_mut();
    state.digit = 4;
    for (index, slot) in slots.iter_mut().enumerate() {
        slot.write(Owner {
            drops: &drops,
            digit: index + 1,
        });
    }
    // SAFETY: every slot owns one initialized Owner and none has been extracted.
    let (block, activity) =
        Tracker::measure(|| unsafe { alloc::raw::AssumeInit::assume_init(block) });
    assert_eq!(activity, [0; 4]);
    assert_eq!(block.as_ptr(), pointer);
    assert_eq!(*block.extent(), 3);
    assert_eq!(block.state().digit, 4);
    assert_eq!(drops.get(), Some(0));
    let ((), released) = Tracker::measure(move || {
        assert_eq!(block.iter().map(|value| value.digit).sum::<usize>(), 6);
    });
    assert_eq!(drops.get(), Some(1234));
    assert_eq!(released, [0, 0, 1, 3 * mem::size_of::<Owner<'_>>()]);
}

#[test]
fn block_zero_size() {
    let mut calls = 0;
    let ((), activity) = Tracker::measure(|| {
        let empty = array::Block::try_new(0_usize, ())
            .expect("empty allocation")
            .init(|_| {
                calls += 1;
                0_u64
            });
        assert!(empty.is_empty());
        assert_eq!(calls, 0);
        let zst = array::Block::try_new(7_usize, ())
            .expect("zero-sized allocation")
            .init(|index| {
                assert_eq!(index, calls);
                calls += 1;
                AlignedZst
            });
        assert_eq!(zst.len(), 7);
        assert_eq!(zst.as_ptr().addr() % mem::align_of::<AlignedZst>(), 0);
    });
    assert_eq!(calls, 7);
    assert_eq!(activity, [0; 4]);
}

#[test]
fn block_vec_owners() {
    let drops = cell::Cell::new(Some(0));
    let block = array::Block::try_new(3_usize, ())
        .expect("owner allocation")
        .init(|index| Owner {
            drops: &drops,
            digit: index + 1,
        });
    let pointer = block.as_ptr();
    let (values, transfer) = Tracker::measure(|| block.into_vec());
    assert_eq!(transfer, [0; 4]);
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!((values.len(), values.capacity()), (3, 3));
    assert_eq!(drops.get(), Some(0));

    let ((), released) = Tracker::measure(|| drop(values));
    assert_eq!(drops.get(), Some(123));
    assert_eq!(released, [0, 0, 1, 3 * mem::size_of::<Owner<'_>>()]);
}

#[test]
fn block_vec_layouts() {
    let block = array::Block::try_new(3_usize, ())
        .expect("aligned allocation")
        .init(|index| Aligned(index as u8));
    let pointer = block.as_ptr();
    let (values, transfer) = Tracker::measure(|| block.into_vec());
    assert_eq!(transfer, [0; 4]);
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.as_ptr().addr() % mem::align_of::<Aligned>(), 0);
    assert_eq!((values.len(), values.capacity()), (3, 3));
    for (index, value) in values.iter().enumerate() {
        assert_eq!(value.0, index as u8);
    }
    let ((), released) = Tracker::measure(|| drop(values));
    assert_eq!(released, [0, 0, 1, 3 * mem::size_of::<Aligned>()]);

    let ((), activity) = Tracker::measure(|| {
        let empty = array::Block::try_new(0_usize, ())
            .expect("empty backing")
            .repeat_with(|| Aligned(0));
        let pointer = empty.as_ptr();
        let empty = empty.into_vec();
        assert_eq!(empty.as_ptr(), pointer);
        assert_eq!((empty.len(), empty.capacity()), (0, 0));

        let zst = array::Block::try_new(7_usize, ())
            .expect("zero-sized backing")
            .repeat_with(|| AlignedZst);
        let pointer = zst.as_ptr();
        let zst = zst.into_vec();
        assert_eq!(zst.as_ptr(), pointer);
        assert_eq!(zst.as_ptr().addr() % mem::align_of::<AlignedZst>(), 0);
        assert_eq!((zst.len(), zst.capacity()), (7, usize::MAX));

        let maximum = array::Block::<mem::MaybeUninit<AlignedZst>, usize>::try_new(usize::MAX, ())
            .expect("maximum zero-sized backing")
            .into_vec();
        assert_eq!(
            (maximum.len(), maximum.capacity()),
            (usize::MAX, usize::MAX)
        );
        assert_eq!(maximum.as_ptr().addr() % mem::align_of::<AlignedZst>(), 0);
    });
    assert_eq!(activity, [0; 4]);
}

#[test]
fn slice_plan() {
    let plan = {
        let source = [Aligned(3), Aligned(7)];
        array::Plan::from_slice(&source)
    };
    assert!(matches!(
        Tracker::fail_after(0, || plan.try_vec()),
        Err(alloc::Exhausted)
    ));
    let (values, activity) = Tracker::measure(|| plan.try_vec());
    let values = values.expect("retained source layout");
    assert!(values.is_empty());
    assert_eq!(values.capacity(), 2);
    assert_eq!(values.as_ptr().addr() % mem::align_of::<Aligned>(), 0);
    let bytes = 2 * mem::size_of::<Aligned>();
    assert_eq!(activity, [1, bytes, 0, 0]);
    let ((), released) = Tracker::measure(|| drop(values));
    assert_eq!(released, [0, 0, 1, bytes]);
}

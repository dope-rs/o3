pub mod common;

use std::{cell, hint, mem, num, rc::Rc};

use common::{
    Counted,
    alloc::{
        AlignedUnit, AlignedZst, EmptyRows, Move, Plan, Rows, Wrapped,
        owner::{Aligned, Owner},
        system::{Activity, Tracker},
    },
};
use o3::alloc::{self, raw::Shape as _};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn planes_own_roots() {
    let drops = cell::Cell::new(Some(0));
    let calls = cell::Cell::new(0);
    let owner = Rc::new(());
    let weak = Rc::downgrade(&owner);
    let plan = Plan::<Owner<'_>, u64>::try_new(3, 2).expect("representable plane layouts");
    let bytes = plan.layout().size() + 4 * mem::size_of::<usize>();
    let ((), activity) = Tracker::measure(|| {
        let drops = &drops;
        let calls = &calls;
        let block = alloc::raw::Block::try_from_fn(
            plan,
            Owner {
                drops,
                value: Box::new(9),
            },
            move |index| {
                assert_eq!(Rc::strong_count(&owner), 1);
                calls.set(calls.get() + 1);
                Owner {
                    drops,
                    value: Box::new(index + 1),
                }
            },
            |index| {
                assert!(weak.upgrade().is_some());
                calls.set(calls.get() + 1);
                index as u64 + 10
            },
        )
        .expect("two initialized planes");
        assert!(weak.upgrade().is_none());
        assert_eq!(*block.as_ref()[2].value, 3);
        assert_eq!(block.tail(), [10, 11]);
    });
    assert_eq!(calls.get(), 5);
    assert_eq!(drops.get(), Some(1239));
    assert_eq!(activity, [5, bytes, 5, bytes]);
}

#[test]
fn planes_parts() {
    let calls = cell::Cell::new(0);
    let bytes = alloc::raw::Shape::layout(&Rows).size();
    let ((), activity) = Tracker::measure(|| {
        let block = alloc::raw::Block::try_parts(Rows, (), |index| {
            calls.set(calls.get() + 1);
            (index as u8, index as u16 + 10)
        })
        .expect("four-plane allocation");
        assert_eq!(block.extra(), (&[0, 1, 2][..], &[10, 11, 12][..]));
    });
    assert_eq!(calls.get(), 3);
    assert_eq!(activity, [1, bytes, 1, bytes]);
}

#[test]
fn row_bounds_match_layout() {
    use std::alloc::{Layout, LayoutError};

    fn check<R, T, E, L>() {
        fn layout<R, T, E, L>(count: usize) -> Result<Layout, LayoutError> {
            let root = Layout::array::<R>(count)?;
            let (layout, _) = root.extend(Layout::array::<T>(count)?)?;
            let (layout, _) = layout.extend(Layout::array::<E>(count)?)?;
            let (layout, _) = layout.extend(Layout::array::<L>(count)?)?;
            Ok(layout.pad_to_align())
        }

        let maximum = <EmptyRows<R, T, E, L> as alloc::raw::Rows>::EXTRA_MAX;
        assert!(layout::<R, T, E, L>(maximum).is_ok());
        if let Some(overflow) = maximum.checked_add(1) {
            assert!(layout::<R, T, E, L>(overflow).is_err());
        }
    }

    check::<u32, u64, u8, u16>();
    check::<u8, Aligned, u16, AlignedZst>();
    check::<Aligned, u8, AlignedZst, u16>();
    check::<u8, u16, Aligned, AlignedZst>();
    check::<u8, u16, AlignedZst, Aligned>();
    check::<(), (), (), ()>();
    check::<AlignedZst, (), AlignedZst, AlignedZst>();
    assert_eq!(
        <EmptyRows<AlignedZst, (), AlignedZst, AlignedZst> as alloc::raw::Rows>::EXTRA_MAX,
        usize::MAX
    );
}

#[test]
fn planes_own_extra() {
    let drops = cell::Cell::new(Some(0));
    let calls = cell::Cell::new(0);
    let owner = Rc::new(());
    let weak = Rc::downgrade(&owner);
    let plan =
        Plan::<Owner<'_>, u64, Aligned>::try_new(3, 2).expect("representable three-plane layout");
    let bytes = plan.layout().size() + 3 * mem::size_of::<usize>();
    let ((), activity) = Tracker::measure(|| {
        let drops = &drops;
        let calls = &calls;
        let mut block = alloc::raw::Block::try_planes(
            plan,
            (),
            move |index| {
                assert_eq!(Rc::strong_count(&owner), 1);
                calls.set(calls.get() + 1);
                (
                    Owner {
                        drops,
                        value: Box::new(index + 1),
                    },
                    Aligned(index as u8 + 20),
                )
            },
            |index| {
                assert!(weak.upgrade().is_some());
                assert_eq!(calls.get(), 3);
                index as u64 + 10
            },
        )
        .expect("initialized three-plane owner");
        assert!(weak.upgrade().is_none());
        assert_eq!(*block.as_ref()[2].value, 3);
        assert_eq!(block.tail(), [10, 11]);
        assert_eq!(block.values().len(), 3);
        assert_eq!(
            [
                block.values()[0].0,
                block.values()[1].0,
                block.values()[2].0
            ],
            [20, 21, 22]
        );
        assert_eq!(block.values().as_ptr().addr() % align_of::<Aligned>(), 0);
        block.parts_mut().2[1] = 29;
        assert_eq!(block.tail(), [10, 29]);
        assert_eq!(block.values()[1].0, 21);
    });
    assert_eq!(drops.get(), Some(123));
    assert_eq!(activity, [4, bytes, 4, bytes]);
}

#[test]
fn planes_extra_failure() {
    let calls = cell::Cell::new(0);
    let plan = Plan::<u8, u64, Aligned>::try_new(3, 2).expect("representable three-plane layout");
    let (result, activity) = Tracker::activity(|| {
        Tracker::fail_after(0, || {
            alloc::raw::Block::try_planes(
                plan,
                (),
                |index| {
                    calls.set(calls.get() + 1);
                    (index as u8, Aligned(0))
                },
                |_| {
                    calls.set(calls.get() + 1);
                    0
                },
            )
        })
    });
    assert!(matches!(result, Err(alloc::Exhausted)));
    assert_eq!(calls.get(), 0);
    assert!(activity == Activity::default());
}

#[test]
fn planes_extra_empty() {
    let plan =
        Plan::<u8, u64, Aligned>::try_new(0, 2).expect("empty root with aligned auxiliary plane");
    let block = alloc::raw::Block::try_planes(plan, (), |_| (0, Aligned(0)), |index| index as u64)
        .expect("tail-only allocation");
    assert!(block.as_ref().is_empty());
    assert!(block.values().is_empty());
    assert_eq!(block.tail(), [0, 1]);
}

#[test]
fn planes_zero_size() {
    let calls = cell::Cell::new(0);
    let ((), activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            let block = alloc::raw::Block::try_from_fn(
                Plan::<u64, u64>::try_new(0, 0).expect("representable plane layouts"),
                (),
                |_| {
                    calls.set(calls.get() + 1);
                    0
                },
                |_| {
                    calls.set(calls.get() + 1);
                    0
                },
            )
            .expect("empty planes");
            assert!(block.as_ref().is_empty());
            assert!(block.tail().is_empty());
            let block = alloc::raw::Block::try_from_fn(
                Plan::<(), ()>::try_new(3, 2).expect("representable plane layouts"),
                (),
                |_| (),
                |_| (),
            )
            .expect("zero-sized planes");
            assert_eq!(block.as_ref().len(), 3);
            assert_eq!(block.tail().len(), 2);
        });
    });
    assert_eq!(calls.get(), 0);
    assert_eq!(activity, [0; 4]);
}

#[test]
fn block_extent() {
    use o3::alloc::array::Block;

    let drops = cell::Cell::new(Some(0));
    let (released, bytes) = {
        let (block, activity) = Tracker::activity(|| {
            Block::<mem::MaybeUninit<u64>, _, _>::try_new(7_usize, Counted(&drops))
        });
        let block = block.expect("seven-entry block allocation");
        assert_eq!(block.len(), 7);
        assert_eq!(block.as_ptr().addr() % align_of::<u64>(), 0);
        assert_eq!(activity.allocations, 1);
        assert_eq!(activity.allocated_bytes, 7 * size_of::<u64>());
        assert_eq!(drops.get(), Some(0));
        (Tracker::begin(), activity.allocated_bytes)
    };
    let released = released.finish();
    assert_eq!(drops.get(), Some(1));
    assert_eq!(released.deallocations, 1);
    assert_eq!(released.deallocated_bytes, bytes);
}

#[test]
fn block_recovery() {
    use o3::alloc::array::Block;

    let state = Rc::new(());
    let (result, activity) = Tracker::activity(|| {
        Tracker::fail_after(0, || {
            Block::<mem::MaybeUninit<u64>, _, _>::try_new(7_usize, state.clone())
        })
    });
    assert!(matches!(result, Err(alloc::Error::Exhausted)));
    assert_eq!(Rc::strong_count(&state), 1);
    assert!(activity == Activity::default());
    let (overflow, activity) = Tracker::activity(|| {
        Block::<mem::MaybeUninit<u64>, _, _>::try_new(usize::MAX, state.clone())
    });
    assert!(matches!(overflow, Err(alloc::Error::Overflow)));
    assert_eq!(Rc::strong_count(&state), 1);
    assert!(activity == Activity::default());
}

#[test]
fn plan_reuse() {
    use o3::alloc::array::Plan;

    let len = hint::black_box(7_usize);
    let (plan, activity) =
        Tracker::activity(|| Tracker::fail_after(0, || Plan::<u64, _>::try_new(len)));
    let plan = plan.expect("representable layout needs no allocation");
    assert!(activity == Activity::default());
    let mut first = plan.try_allocate(()).expect("first allocation");
    first[6].write(29);
    let (failed, activity) = Tracker::activity(|| Tracker::fail_after(0, || plan.try_allocate(())));
    assert!(matches!(failed, Err(alloc::Exhausted)));
    assert!(activity == Activity::default());
    let second = plan.try_allocate(()).expect("retry preserves the plan");
    assert_eq!(second.len(), first.len());
    assert_ne!(second.as_ptr(), first.as_ptr());
    assert_eq!(unsafe { first[6].assume_init() }, 29);
}

#[test]
fn plan_limits() {
    use o3::alloc::array::Plan;

    let limit = isize::MAX as usize / size_of::<u64>();
    let ((), activity) = Tracker::activity(|| {
        assert!(Plan::<u64, _>::try_new(limit).is_ok());
        assert!(matches!(
            Plan::<u64, _>::try_new(limit + 1),
            Err(alloc::Overflow)
        ));
        assert!(matches!(
            Plan::<u64, _>::try_new(usize::MAX),
            Err(alloc::Overflow)
        ));
    });
    assert!(activity == Activity::default());
    let block = Plan::<u64, _>::try_new(num::NonZero::<u32>::MIN.saturating_add(6))
        .expect("representable extent")
        .try_allocate(())
        .expect("admitted layout allocation");
    assert_eq!(*block.extent(), num::NonZero::<u32>::MIN.saturating_add(6));
    assert_eq!(size_of_val(&block), 2 * size_of::<usize>());
}

#[test]
fn block_zero_size() {
    use o3::alloc::array::Block;

    let ((), activity) = Tracker::activity(|| {
        Tracker::fail_after(0, || {
            let empty =
                Block::<mem::MaybeUninit<u64>, _>::try_new(0_usize, ()).expect("empty block");
            let zst = Block::<mem::MaybeUninit<AlignedUnit>, _>::try_new(usize::MAX, ())
                .expect("zero-sized block");
            assert!(empty.is_empty());
            assert_eq!(zst.len(), usize::MAX);
            assert_eq!(zst.as_ptr().addr() % align_of::<AlignedUnit>(), 0);
        });
    });
    assert!(activity == Activity::default());
}

#[test]
fn wrapped_migration() {
    use alloc::array::Block;

    let (failed, activity) = Tracker::activity(|| Tracker::fail_after(4, Wrapped::try_new));
    assert!(matches!(failed, Err(alloc::Error::Exhausted)));
    assert_eq!(activity.allocations, 4);
    assert_eq!(activity.deallocations, 4);
    assert_eq!(activity.allocated_bytes, activity.deallocated_bytes);

    let released = {
        let mut owner = Wrapped::try_new().expect("wrapped allocation");
        let prior = owner.block().as_ptr();
        let short = Block::try_new(3, ()).expect("short allocation");
        let Err((required, short)) = Move::try_new(&mut owner, short) else {
            panic!("short target rejected");
        };
        assert_eq!((required, short.len()), (4, 3));
        drop(short);
        let spare = Block::try_new(8, ()).expect("cancelled allocation");
        drop(Move::try_new(&mut owner, spare).expect("cancelled target fits"));
        assert_eq!(owner.block().as_ptr(), prior);
        assert_eq!(owner.values(), [10, 20, 30, 40]);

        let spare = Block::try_new(8, ()).expect("replacement allocation");
        let ((), trace) = Tracker::activity(|| {
            Move::try_new(&mut owner, spare)
                .expect("target fits")
                .commit()
        });
        assert_eq!(trace.allocations, 0);
        assert_eq!(trace.deallocations, 1);
        assert_eq!(*owner.block().state(), (0, 4));
        assert_eq!(owner.values(), [10, 20, 30, 40]);
        Tracker::begin()
    };
    let trace = released.finish();
    assert_eq!(trace.allocations, 0);
    assert_eq!(trace.deallocations, 5);
}

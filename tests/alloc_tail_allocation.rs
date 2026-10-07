pub mod common;

use std::{cell::Cell, mem, ops, ptr};

use common::alloc::{
    Aligned, Owner,
    system::Tracker,
    tail::{Entry, Header},
};
use o3::alloc::{self, tail};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn initializes_exact_tail() {
    for count in [0, 3] {
        let drops = Cell::new(Some(0));
        let observed = Cell::new(None);
        let plan = tail::Plan::<Header<'_>, Entry<'_>>::try_new(count).expect("tail layout");
        let bytes = (count + 1) * mem::size_of::<Entry<'_>>();
        let freed = {
            let (memory, cold) = Tracker::measure(|| plan.try_allocate());
            assert_eq!(cold, [1, bytes, 0, 0]);
            let memory = memory.expect("header and exact tail allocation");
            let (block, init) = Tracker::measure(|| {
                memory.fill(
                    Header {
                        drops: &drops,
                        observed: &observed,
                    },
                    |_, index| Entry::new(index),
                )
            });
            assert_eq!(init, [0; 4]);
            let values = tail::View::new(&block).as_slice();
            assert_eq!(values.len(), count);
            assert_eq!(values.as_ptr().addr() % 128, 0);
            for (index, slot) in values.iter().enumerate() {
                slot.drops.set(Some(&drops));
                assert_eq!(slot.index, index);
            }
            Tracker::begin()
        };
        assert_eq!(
            freed.finish(),
            common::alloc::system::Activity {
                allocations: 0,
                allocated_bytes: 0,
                deallocations: 1,
                deallocated_bytes: bytes
            }
        );
        assert_eq!(drops.get(), Some(count + 1));
        assert_eq!(observed.get(), Some(Some(count)));
    }
}

#[test]
fn failure_is_inert() {
    let plan = tail::Plan::<Header<'_>, Entry<'_>>::fixed::<3>();
    let (memory, activity) = Tracker::measure(|| Tracker::fail_after(0, || plan.try_allocate()));
    assert!(matches!(memory, Err(alloc::Exhausted)));
    assert_eq!(activity, [0; 4]);
}

#[test]
fn recovers_aligned_owner() {
    use std::num::NonZero;

    let drops = Cell::new(Some(0));
    let observed = Cell::new(None);
    {
        let count = NonZero::new(3u32).expect("nonzero tail");
        let block = tail::Plan::<Header<'_>, Entry<'_>, _>::try_new(count)
            .expect("aligned layout")
            .try_allocate()
            .expect("tail allocation")
            .fill(
                Header {
                    drops: &drops,
                    observed: &observed,
                },
                |_, index| Entry::new(index as usize),
            );
        for index in 0..count.get() {
            // SAFETY: the loop visits each exact initialized resident once.
            let resident = unsafe { alloc::raw::Resident::new(tail::View::new(&block), index) };
            let entry = resident.get();
            entry.drops.set(Some(&drops));
            entry.value.set(index as usize + 7);
            // SAFETY: each entry stores its immutable original construction ordinal.
            let view = unsafe { resident.view(entry.index as u32) };
            assert!(std::ptr::eq(
                view.header(),
                tail::View::new(&block).header()
            ));
            assert!(std::ptr::eq(
                view.as_slice(),
                tail::View::new(&block).as_slice()
            ));
            assert_eq!(
                view.as_slice()[index as usize].value.get(),
                index as usize + 7
            );
        }
    }
    assert_eq!(drops.get(), Some(4));
    assert_eq!(observed.get(), Some(Some(3)));
}

#[test]
fn layout_is_exact() {
    const LEN: usize = 257;
    const BYTES: usize = (2 + LEN) * mem::size_of::<u64>();
    assert_eq!(
        mem::size_of::<tail::Entry<u64, u64>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<ops::ControlFlow<tail::Block<u64, u64>, tail::Entry<u64, u64>>>(),
        2 * mem::size_of::<usize>(),
    );
    let released = {
        let (memory, cold) =
            Tracker::measure(|| tail::Plan::<u64, u64>::fixed::<LEN>().try_allocate());
        assert_eq!(cold, [1, BYTES, 0, 0]);
        let memory = memory.expect("exact tail allocation");
        let (block, hot) = Tracker::measure(|| {
            let mut init = memory.start(7);
            loop {
                match init {
                    ops::ControlFlow::Break(block) => break block,
                    ops::ControlFlow::Continue(entry) => {
                        let index = entry.index();
                        init = entry.insert(index as u64);
                    }
                }
            }
        });
        assert_eq!(hot, [0; 4]);
        assert_eq!(*tail::View::new(&block).header(), 7);
        assert_eq!(tail::View::new(&block).as_slice().len(), LEN);
        assert_eq!(tail::View::new(&block).as_slice()[17], 17);
        Tracker::begin()
    };
    assert_eq!(
        released.finish(),
        common::alloc::system::Activity {
            allocations: 0,
            allocated_bytes: 0,
            deallocations: 1,
            deallocated_bytes: BYTES
        }
    );
}

#[test]
fn drops_prefix_then_header() {
    for count in 0..=3 {
        let drops = Cell::new(Some(0));
        {
            let memory = tail::Plan::<Owner<'_>, Owner<'_>>::fixed::<3>()
                .try_allocate()
                .expect("three values and their header");
            let mut init = memory.start(Owner {
                digit: 9,
                drops: &drops,
            });
            for id in 0..count {
                let entry = init.continue_value().expect("next uninitialized entry");
                assert_eq!(entry.index(), id);
                init = entry.insert(Owner {
                    digit: id + 1,
                    drops: &drops,
                });
            }
            assert_eq!(init.is_break(), count == 3);
            assert_eq!(drops.get(), Some(0));
        }
        assert_eq!(drops.get(), Some([9, 19, 129, 1239][count]));
    }
}

#[test]
fn preserves_header_address() {
    let memory = tail::Plan::<u64, ptr::NonNull<u64>>::fixed::<3>()
        .try_allocate()
        .expect("header with three resident pointers");
    let mut init = memory.start(23);
    let block = loop {
        match init {
            ops::ControlFlow::Break(block) => break block,
            ops::ControlFlow::Continue(entry) => {
                let header = ptr::NonNull::from(entry.header());
                init = entry.insert(header);
            }
        }
    };
    for pointer in tail::View::new(&block).as_slice() {
        assert_eq!(
            *pointer,
            ptr::NonNull::from(tail::View::new(&block).header())
        );
    }
}

#[test]
fn preserves_alignment() {
    for count in [0, 3] {
        let memory = tail::Plan::<u8, Aligned>::try_new(count)
            .expect("small aligned layout")
            .try_allocate()
            .expect("aligned allocation");
        let mut init = memory.start(11);
        let block = loop {
            match init {
                ops::ControlFlow::Break(block) => break block,
                ops::ControlFlow::Continue(entry) => {
                    let index = entry.index();
                    init = entry.insert(Aligned(index as u8));
                }
            }
        };
        assert_eq!(tail::View::new(&block).as_slice().as_ptr() as usize % 64, 0);
        assert!(
            tail::View::new(&block)
                .as_slice()
                .iter()
                .map(|value| value.0)
                .eq(0..count as u8)
        );
    }
}

#[test]
fn initializes_zero_sizes() {
    for count in [0, 17] {
        let memory = tail::Plan::<u8, ()>::try_new(count)
            .expect("zero-sized layout")
            .try_allocate()
            .expect("header allocation");
        let mut init = memory.start(4);
        let mut written = 0;
        let block = loop {
            match init {
                ops::ControlFlow::Break(block) => break block,
                ops::ControlFlow::Continue(entry) => {
                    assert_eq!(entry.index(), written);
                    written += 1;
                    init = entry.insert(());
                }
            }
        };
        assert_eq!(written, count);
        assert_eq!(tail::View::new(&block).as_slice().len(), count);
    }
}

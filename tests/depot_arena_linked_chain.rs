pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, num::NonZero, ops, ptr};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::arena::linked::{
        Tag,
        reserve::{Operation, Script},
    },
};
use o3::{
    alloc::tail,
    depot::{
        self,
        arena::linked::{Batch as _, chain, fifo, head, slots},
        capacity,
    },
};

#[test]
fn reuses_one_admission() {
    let drops = Cell::new(Some(0));
    let nodes = (depot::raw::Arena::try_new(1)).expect("one node");
    let mut head = head::List::new();
    let (result, counts) = Tracker::measure(|| {
        let script = unsafe {
            Script::try_new(
                &nodes,
                &mut head,
                [
                    Operation::Push(Counted(&drops)),
                    Operation::Pop,
                    Operation::Push(Counted(&drops)),
                    Operation::Pop,
                    Operation::Push(Counted(&drops)),
                    Operation::Pop,
                ],
            )
        }
        .expect("fixture script must be valid");
        script.try_apply()
    });
    assert!(
        matches!(result, Ok(Ok(()))),
        "script fits one node and topology"
    );
    assert_eq!(counts, [0; 4]);
    assert_eq!(drops.get(), Some(3));
    assert_eq!(nodes.available(), 1);
}

#[test]
fn recycles_full_store() {
    let nodes = (depot::raw::Arena::try_new(1)).expect("one node");
    let mut head = head::List::new();
    let slot = (nodes.vacant()).expect("initial vacancy");
    unsafe {
        let mut cursor = nodes.cursor(&mut head);
        slots::Cursor::insert(depot::raw::Proof::new((&mut cursor, slot.insert(17))));
    }
    assert_eq!(nodes.available(), 0);
    let script =
        unsafe { Script::try_new(&nodes, &mut head, [Operation::Pop, Operation::Push(23)]) }
            .expect("fixture script must be valid");
    assert!(
        matches!(script.try_apply(), Ok(Ok(()))),
        "recycled node and topology"
    );
    assert_eq!(nodes.available(), 0);
    assert_eq!(unsafe { nodes.pop_front(&mut head) }, Some(23));
    assert_eq!(nodes.available(), 1);
}

#[test]
fn shortage_keeps_inputs() {
    let drops = Cell::new(Some(0));
    let nodes = (depot::raw::Arena::try_new(1)).expect("one node");
    let mut head = head::List::new();
    {
        let script = unsafe {
            Script::try_new(
                &nodes,
                &mut head,
                [
                    Operation::Push(Counted(&drops)),
                    Operation::Push(Counted(&drops)),
                ],
            )
        }
        .expect("fixture script must be valid");
        let (script, error) = script.try_apply().expect_err("two simultaneous nodes");
        assert_eq!(
            error,
            capacity::Shortage {
                requested: NonZero::<usize>::MIN.saturating_add(1),
                available: 1,
            }
        );
        assert_eq!(drops.get(), Some(0));
        assert_eq!(nodes.available(), 1);
        assert!(unsafe { nodes.iter(script.head()) }.next().is_none());
    }
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn retains_detached_slot() {
    let nodes = (depot::raw::Arena::<i32>::try_new(1)).expect("one node");
    let mut head = head::List::new();
    let slot = (nodes.vacant()).expect("initial vacancy");
    unsafe {
        let mut cursor = nodes.cursor(&mut head);
        slots::Cursor::insert(depot::raw::Proof::new((&mut cursor, slot.insert(17))));
    }
    let entry = (unsafe { nodes.front_entry(&mut head) }).expect("one live node");
    let (value, slot) = entry.take();
    assert_eq!(value, 17);
    assert!(unsafe { nodes.front_entry(&mut head) }.is_none());
    assert_eq!(nodes.available(), 0);
    unsafe {
        let mut cursor = nodes.cursor(&mut head);
        slots::Cursor::insert(depot::raw::Proof::new((&mut cursor, slot.insert(23))));
    }
    assert_eq!(unsafe { nodes.pop_front(&mut head) }, Some(23));
    assert_eq!(nodes.available(), 1);
}

#[test]
fn retains_links() {
    let owner = chain::List::new();
    let memory = tail::Plan::try_new(2)
        .expect("layout")
        .try_allocate()
        .expect("first allocation");
    let first = owner.push(memory.repeat(chain::Node::new(7), 7));
    let header = first.header();
    let entries = first.as_slice();
    let mut old = owner.iter();
    let block = tail::Plan::try_new(3)
        .expect("layout")
        .try_allocate()
        .expect("prepared allocation")
        .fill(chain::Node::new(11), |header, index| {
            header.header() + index
        });
    let (second, hot) = Tracker::measure(|| owner.push(block));
    assert_eq!(hot, [0; 4]);
    assert_eq!(second.as_slice(), [11, 12, 13]);
    assert_eq!(second.len(), 3);
    assert!(ptr::eq(header, first.header()));
    assert!(ptr::eq(entries.as_ptr(), first.as_slice().as_ptr()));
    assert_eq!(entries, [7, 7]);
    assert_eq!(old.next().expect("retained iterator head").header(), &7);
    assert!(old.next().is_none());
    assert!(owner.iter().map(|link| *link.header()).eq([11, 7]));
}

#[test]
fn interleaves_insertions() {
    let owner = chain::List::new();
    let memory = tail::Plan::try_new(1)
        .expect("layout")
        .try_allocate()
        .expect("initial allocation");
    let first = owner.push(memory.repeat(chain::Node::new(3), 3));
    let memory = tail::Plan::try_new(2)
        .expect("layout")
        .try_allocate()
        .expect("outer allocation");
    let mut init = memory.start(chain::Node::new(7));
    let block = loop {
        match init {
            ops::ControlFlow::Break(block) => break block,
            ops::ControlFlow::Continue(entry) => {
                let value = *entry.header().header();
                let memory = tail::Plan::try_new(1)
                    .expect("layout")
                    .try_allocate()
                    .expect("inner allocation");
                owner.push(memory.repeat(chain::Node::new(value + 1), value + 1));
                assert_eq!(first.as_slice(), [3]);
                init = entry.insert(value);
            }
        }
    };
    let outer = owner.push(block);
    assert_eq!(outer.as_slice(), [7, 7]);
    assert!(owner.iter().map(|link| *link.header()).eq([7, 8, 8, 3]));
}

#[test]
fn frees_deep_chain() {
    let (cold, freed) = {
        let build = Tracker::begin();
        let owner = chain::List::new();
        for header in 0..32_768 {
            let memory = tail::Plan::try_new(1)
                .expect("layout")
                .try_allocate()
                .expect("chain allocation");
            owner.push(memory.repeat(chain::Node::new(header), header));
        }
        let cold = build.finish();
        assert_eq!(cold.allocations, 32_768);
        (cold, Tracker::begin())
    };
    let freed = freed.finish();
    assert_eq!(freed.allocations, 0);
    assert_eq!(freed.allocated_bytes, 0);
    assert_eq!(freed.deallocations, cold.allocations);
    assert_eq!(freed.deallocated_bytes, cold.allocated_bytes);
}

#[test]
fn carries_claim_identity() {
    let mut pool = depot::raw::Arena::<_, Tag>::try_new(1).expect("one-slot linked permit pool");
    let mut queue = fifo::Fifo::new();

    {
        let claimed = pool.permit().map(|permit| permit.claim(5));
        assert!(claimed.is_some(), "the new pool must expose its one permit");
    }
    assert_eq!(pool.available(), 1);

    let claim = pool.permit().expect("vacant slot").claim(7);
    // SAFETY: this claim and the empty FIFO belong to this exact pool.
    slots::Live::push_back(unsafe { depot::raw::Proof::new((claim, &mut queue)) });
    assert_eq!(pool.available(), 0);
    assert!(pool.permit().is_none());
    *unsafe { pool.back_mut(queue.back().expect("live tail")) } = 11;
    assert_eq!(unsafe { pool.queue(&mut queue) }.pop_front(), Some(11));

    let claim = pool.permit().expect("recycled slot").claim(13);
    // SAFETY: popping returned the FIFO's only slot to this same pool.
    slots::Live::push_back(unsafe { depot::raw::Proof::new((claim, &mut queue)) });
    assert_eq!(unsafe { pool.queue(&mut queue) }.pop_front(), Some(13));
    assert_eq!(pool.available(), 1);
}

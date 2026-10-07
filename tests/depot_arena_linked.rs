pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{
    cell::{self, Cell},
    collections::VecDeque,
    mem::{self, size_of},
};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{
        arena::linked::{Branded, Head, NONE, Pool, Slots, Tag},
        model::Random,
    },
};
use o3::depot::{
    self,
    arena::linked::{
        self,
        fifo::{self, ends},
        head,
    },
    raw::Link as _,
};

#[test]
fn raw_head_keeps_links() {
    let links = [Cell::new(1), Cell::new(NONE), Cell::new(NONE)];
    let head = head::Cell::new(0);

    let first = head.front();
    let first = first.expect("assertion failed: first.is_some()");
    let first = unsafe { depot::raw::HeadPop::apply(Slots(&links), first, "first") };
    assert_eq!(first, (0, "first"));
    assert_eq!(head.front().map(|front| front.item()), Some(1));

    unsafe { depot::raw::HeadPush::apply(Slots(&links), &head, 2) };
    assert_eq!(head.front().map(|front| front.item()), Some(2));

    let second = head.front();
    let second = second.expect("assertion failed: second.is_some()");
    assert_eq!(
        unsafe { depot::raw::HeadPop::apply(Slots(&links), second, 7) },
        (2, 7)
    );

    let third = head.front();
    let third = third.expect("assertion failed: third.is_some()");
    assert_eq!(
        unsafe { depot::raw::HeadPop::apply(Slots(&links), third, 8) },
        (1, 8)
    );
    assert!(head.front().is_none());
}

#[test]
fn exclusive_transition() {
    let links = [Cell::new(NONE), Cell::new(NONE)];
    let mut head = head::Exclusive::new(NONE);
    unsafe { depot::raw::HeadPush::apply(Slots(&links), &mut head, 0) };
    unsafe { depot::raw::HeadPush::apply(Slots(&links), &mut head, 1) };
    assert_eq!(links[1].get(), 0);
    let front = head.front().expect("published head");
    assert_eq!(
        unsafe { depot::raw::HeadPop::apply(Slots(&links), front, 17) },
        (1, 17)
    );
    assert_eq!(head.raw(), 0);
    assert_eq!(links[1].get(), NONE);
    let front = head.front().expect("remaining head");
    assert_eq!(
        unsafe { depot::raw::HeadPop::apply(Slots(&links), front, ()) },
        (0, ())
    );
    assert!(head.front().is_none());
}

#[test]
fn keeps_affine_chains() {
    let links = [const { Cell::new(NONE) }; 8];
    let queue = ends::Cell::empty();
    let mut first = ends::Exclusive::empty();

    // SAFETY: both indices are detached in these links, and first starts empty.
    unsafe {
        Slots(&links).push_back(&mut first, 1);
        Slots(&links).push_back(&mut first, 2);
    }
    assert_eq!(first.head(), Some(1));

    unsafe {
        ends::Cell::append(depot::raw::Proof::new((
            &queue,
            &mut first,
            |tail, head| {
                assert_eq!(tail, None);
                assert_eq!(head, 1);
            },
        )));
    }
    assert!(first.is_empty());
    // SAFETY: append emptied first; slot 4 is detached in these same links.
    unsafe {
        Slots(&links).push_back(&mut first, 4);
        assert_eq!(Slots(&links).pop_front(&mut first), Some(4));
        assert_eq!(Slots(&links).pop_front(&mut first), None);
    }

    unsafe {
        ends::Cell::push_front(depot::raw::Proof::new((&queue, 0, |head| {
            links[0].set(head.unwrap_or(NONE))
        })));
        ends::Cell::push_back(depot::raw::Proof::new((&queue, 3, |tail| {
            links[3].set(NONE);
            links[tail.expect("existing queue tail") as usize].set(3);
        })));
    }

    let mut actual = Vec::new();
    while let Some(front) = queue.front() {
        actual.push(unsafe {
            ends::Front::pop(depot::raw::Proof::new((front, (), |index, ()| {
                (links[index as usize].replace(NONE), index)
            })))
        });
    }
    assert_eq!(actual, [0, 1, 2, 3]);

    assert!(queue.front().is_none());
}

#[test]
fn matches_fifo_churn() {
    const CAPACITY: usize = 32;
    let links = [const { Cell::new(NONE) }; CAPACITY];
    let queue = ends::Cell::empty();
    let mut model = VecDeque::new();
    let mut free = (0..CAPACITY as u32).rev().collect::<Vec<_>>();
    let mut rng = Random::<u32, 13, 17, 5>::try_new(0x9e37_79b9_u32).expect("shifts");

    for turn in 0..20_000 {
        let seed = rng.advance();
        match seed % 5 {
            0 if !free.is_empty() => {
                let index = (free.pop()).expect("nonempty free set");
                unsafe {
                    ends::Cell::push_front(depot::raw::Proof::new((&queue, index, |head| {
                        links[index as usize].set(head.unwrap_or(NONE))
                    })));
                }
                model.push_front(index);
            }
            1 | 2 if !free.is_empty() => {
                let mut chain = ends::Exclusive::empty();
                let count = free.len().min((seed as usize % 4) + 1);
                for _ in 0..count {
                    let index = (free.pop()).expect("nonempty free set");
                    // SAFETY: free supplies detached slots in the same links as chain.
                    unsafe { Slots(&links).push_back(&mut chain, index) };
                    model.push_back(index);
                }
                unsafe {
                    ends::Cell::append(depot::raw::Proof::new((
                        &queue,
                        &mut chain,
                        |tail, head| {
                            if let Some(tail) = tail {
                                links[tail as usize].set(head);
                            }
                        },
                    )));
                }
            }
            3 => {
                let actual = queue.front().map(|front| unsafe {
                    ends::Front::pop(depot::raw::Proof::new((front, (), |index, ()| {
                        (links[index as usize].replace(NONE), index)
                    })))
                });
                let wanted = model.pop_front();
                assert_eq!(actual, wanted, "turn {turn}");
                if let Some(index) = actual {
                    free.push(index);
                }
            }
            _ => {
                let mut next = queue.front();
                while let Some(front) = next {
                    let (index, rest) = unsafe {
                        ends::Front::advance(depot::raw::Proof::new((front, (), |index, ()| {
                            (links[index as usize].replace(NONE), index)
                        })))
                    };
                    assert_eq!(Some(index), model.pop_front(), "turn {turn}");
                    free.push(index);
                    next = rest;
                }
            }
        }
        assert_eq!(queue.head(), model.front().copied(), "turn {turn}");
        assert_eq!(queue.back(), model.back().copied(), "turn {turn}");
    }
}

#[test]
fn retains_appended_front() {
    let links = [Cell::new(NONE), Cell::new(NONE)];
    let queue = ends::Cell::empty();
    unsafe {
        ends::Cell::push_back(depot::raw::Proof::new((&queue, 0, |tail| {
            assert_eq!(tail, None)
        })))
    };
    let first = (queue.front()).expect("retained source");
    unsafe {
        ends::Cell::push_back(depot::raw::Proof::new((&queue, 1, |tail| {
            assert_eq!(tail, Some(0));
            links[0].set(1);
        })));
    }
    let (first, next) = unsafe {
        ends::Front::advance(depot::raw::Proof::new((first, (), |index, ()| {
            (links[index as usize].replace(NONE), index)
        })))
    };
    assert_eq!(first, 0);
    let next = (next).expect("appended successor");
    let (last, next) = unsafe {
        ends::Front::advance(depot::raw::Proof::new((next, (), |index, ()| {
            (links[index as usize].replace(NONE), index)
        })))
    };
    assert_eq!(last, 1);
    assert!(next.is_none());
    assert!(queue.is_empty());
}

#[test]
fn reads_pending_ends() {
    let links = [Cell::new(NONE), Cell::new(NONE)];
    let queue = ends::Cell::empty();
    for index in 0..2 {
        let previous = queue.head();
        // SAFETY: each detached index is linked to this exact chain; observations
        // read the retained endpoints without changing or invalidating them.
        ends::Cell::push_back(unsafe {
            depot::raw::Proof::new((&queue, index, |tail: Option<u32>| {
                assert_eq!(queue.head(), previous);
                assert_eq!(queue.is_empty(), previous.is_none());
                if let Some(tail) = tail {
                    links[tail as usize].set(index);
                }
            }))
        });
        assert_eq!(queue.head(), Some(0));
    }
    for index in 0..2 {
        // SAFETY: the callback consumes the exact front's stored successor and
        // observes the old endpoints before the transition publishes new ones.
        let removed = ends::Cell::pop_front(unsafe {
            depot::raw::Proof::new((&queue, (), |head: u32, ()| {
                assert_eq!(queue.head(), Some(index));
                assert!(!queue.is_empty());
                (links[head as usize].replace(NONE), head)
            }))
        });
        assert_eq!(removed, Some(index));
        assert_eq!(queue.head(), (index == 0).then_some(1));
    }
    assert!(queue.is_empty());
}

#[test]
fn endpoints_need_no_drop() {
    assert!(!mem::needs_drop::<ends::Exclusive>());
    assert!(!mem::needs_drop::<ends::Cell>());
}

#[test]
fn keeps_branded_layout() {
    type Slot = Branded<'static, 7>;

    let links = [const { Cell::new(Slot::NONE) }; 8];
    let queue = ends::Cell::<Slot>::empty();
    let one = Slot::try_new(1).expect("first branded slot");
    let six = Slot::try_new(6).expect("sixth branded slot");

    unsafe {
        ends::Cell::push_back(depot::raw::Proof::new((&queue, one, |tail| {
            assert!((tail).is_none());
            links[one.get() as usize].set(Slot::NONE);
        })));
        ends::Cell::push_back(depot::raw::Proof::new((&queue, six, |tail| {
            assert!((tail) == (Some(one)));
            links[six.get() as usize].set(Slot::NONE);
            links[one.get() as usize].set(six);
        })));
    }

    assert!((queue.head()) == (Some(one)));
    let first = (queue.front()).expect("first branded slot in queue");
    let first = unsafe {
        ends::Front::pop(depot::raw::Proof::new((first, (), |slot, ()| {
            (links[slot.get() as usize].replace(Slot::NONE), slot)
        })))
    };
    let second = (queue.front()).expect("second branded slot in queue");
    let second = unsafe {
        ends::Front::pop(depot::raw::Proof::new((second, (), |slot, ()| {
            (links[slot.get() as usize].replace(Slot::NONE), slot)
        })))
    };
    assert!(([first, second]) == ([one, six]));
    assert!(queue.is_empty());
    assert_eq!(mem::size_of::<Slot>(), mem::size_of::<u32>());
    assert_eq!(
        mem::size_of::<ends::Cell<Slot>>(),
        2 * mem::size_of::<u32>()
    );
}

#[test]
fn skips_forgotten_vacancy() {
    let drops = cell::Cell::new(Some(0));
    {
        let nodes = (depot::raw::Arena::<Counted<&Cell<Option<usize>>>, Tag>::try_new(2))
            .expect("two slots must allocate");
        {
            let first = nodes.vacant().map(|slot| slot.insert(Counted(&drops)));
            assert!(first.is_some(), "first slot");
            let second = nodes.vacant().map(|slot| slot.insert(Counted(&drops)));
            assert!(second.is_some(), "second slot");
        }
        assert_eq!(drops.get(), Some(2));
        let vacant = (nodes.vacant()).expect("recycled slot");
        mem::forget(vacant);
    }
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn layouts_are_compact() {
    assert_eq!(size_of::<head::List<Tag>>(), size_of::<u32>());
    assert_eq!(size_of::<fifo::Fifo<Tag>>(), 3 * size_of::<u32>());
    assert_eq!(size_of::<Pool<u64>>(), 3 * size_of::<usize>());
    if usize::BITS == 64 {
        assert_eq!(size_of::<depot::raw::Arena<[usize; 3]>>(), 24);
        assert_eq!(size_of::<linked::Iter<'static, [usize; 3]>>(), 16);
    }
}

#[test]
fn allocation_is_exact() {
    const CAPACITY: usize = 257;
    const NODE_BYTES: usize = 24;

    let (result, delta) = Tracker::measure(|| Pool::<u64>::try_new(CAPACITY));
    let pool = result.expect("the 257-node linked arena must allocate");
    assert_eq!(delta, [1, CAPACITY * NODE_BYTES, 0, 0]);
    let mut source = Head::new();

    let ((), delta) = Tracker::measure(|| {
        for value in 0..CAPACITY as u64 {
            assert!(unsafe { source.try_insert(&pool, value, value) }.is_ok());
        }
        for value in 0..CAPACITY as u64 {
            assert_eq!(unsafe { source.pop(&pool) }, Some((value, value)));
        }
        assert!(unsafe { source.pop(&pool) }.is_none());
    });
    assert_eq!(delta, [0; 4]);

    let ((), delta) = Tracker::measure(move || assert_eq!(pool.available(), CAPACITY));
    assert_eq!(delta, [0, 0, 1, CAPACITY * NODE_BYTES]);
}

#[test]
fn drops_live_payloads() {
    let counts = [const { cell::Cell::new(Some(0)) }; 3];
    {
        let result = Pool::<Counted<_>>::try_new(3);
        let pool = result.expect("the three-node drop arena must allocate");
        let mut head = Head::new();
        for (index, count) in counts.iter().enumerate() {
            assert!(unsafe { head.try_push(&pool, index as u64, Counted(count),) }.is_ok());
        }
        assert_eq!(pool.available(), 0);
        assert_eq!(
            unsafe { head.iter(&pool) }
                .map(|(key, _)| key)
                .collect::<Vec<_>>(),
            [2, 1, 0]
        );
    }
    assert_eq!(counts.each_ref().map(cell::Cell::get), [Some(1); 3]);
}

#[test]
fn reuses_sorted_slots() {
    let result = Pool::<Box<u64>>::try_new(3);
    let pool = result.expect("the three-node rejection arena must allocate");
    let mut head = Head::new();
    assert!(unsafe { head.try_insert(&pool, 10, Box::new(10u64)) }.is_ok());
    assert!(unsafe { head.try_insert(&pool, 30, Box::new(30u64)) }.is_ok());
    assert!(unsafe { head.try_insert(&pool, 20, Box::new(20u64)) }.is_ok());
    assert_eq!(
        unsafe { head.iter(&pool) }
            .map(|(key, value)| (key, **value))
            .collect::<Vec<_>>(),
        [(10, 10), (20, 20), (30, 30)]
    );
    {
        let (key, value) = unsafe { head.pop(&pool) }.expect("the sorted head must have a front");
        assert_eq!(key, 10);
        assert_eq!(*value, 10);
    }
    assert_eq!(pool.available(), 1);
    assert!(unsafe { head.try_insert(&pool, 5, Box::new(5u64)) }.is_ok());
    assert_eq!(pool.available(), 0);
    assert_eq!(
        unsafe { head.iter(&pool) }
            .map(|(key, value)| (key, **value))
            .collect::<Vec<_>>(),
        [(5, 5), (20, 20), (30, 30)]
    );
}

#[test]
fn fixed_keeps_owner() {
    use common::depot::arena::linked::FixedNode;
    use o3::depot::{arena::linked::slots::fixed::Fixed, array::copy, capacity::Count};

    let counts = Cell::new((0, 0));
    let mut owner = Fixed::<FixedNode<'_>>::try_new(Count::new(3)).expect("three slots");
    let ((), activity) = Tracker::measure(|| {
        assert!(matches!(
            Fixed::<FixedNode<'_>>::try_new(Count::new(1 << 16)),
            Err(o3::alloc::Error::Overflow)
        ));
        assert_eq!(counts.get(), (0, 0));

        let mut inputs = copy::Vec::from([(1, &counts), (2, &counts), (3, &counts), (0, &counts)]);
        let _ = inputs.pop();
        let mut batch = owner.reserve(&inputs).expect("complete reservation");
        let first = batch.take().expect("first publication");
        assert_eq!(first.get(), 1);
        drop(batch);
        assert_eq!(counts.get(), (3, 0));

        let mut inputs = copy::Vec::from([(4, &counts), (5, &counts), (0, &counts)]);
        let _ = inputs.pop();
        let mut batch = owner.reserve(&inputs).expect("unused slots returned");
        let third = batch.take().expect("last refunded slot");
        let second = batch.take().expect("first refunded slot");
        assert_eq!((third.get(), second.get()), (3, 2));
        assert!(batch.take().is_none());
        assert_eq!(batch.get(first).expect("retained publication").value, 1);
        assert_eq!(batch.get(third).expect("reset third").value, 4);
        assert_eq!(batch.get(second).expect("reset second").value, 5);
        drop(batch);
        assert_eq!(counts.get(), (3, 2));

        owner.entry(first).expect("live entry").remove();
        assert!(owner.get(first).is_none());
        assert!(owner.entry(first).is_none());
        assert!(
            owner
                .reserve::<2>(&copy::Vec::from([(6, &counts); 2]))
                .is_none()
        );
        assert_eq!(counts.get(), (3, 2));
        let mut batch = owner
            .reserve::<1>(&copy::Vec::from([(7, &counts)]))
            .expect("failed reservation retained the free tail");
        assert_eq!(batch.take(), Some(first));
        assert_eq!(batch.get(first).expect("reused first").value, 7);
        drop(batch);
        assert_eq!(counts.get(), (3, 3));
    });
    assert_eq!(activity, [0; 4]);
}

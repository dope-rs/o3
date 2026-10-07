pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::num::NonZero;

use common::{
    alloc::system::Tracker,
    buffer::{Bytes as _, Fill as _, try_item, try_many},
};
use o3::{
    alloc,
    buffer::{
        Residency as _,
        raw::{Slice, Store},
        segments::{self, Drain, assembly, queue},
        storage,
        write::consume::Consume as _,
    },
    depot::capacity::{self, Count},
};

const _: () = {
    assert!(
        size_of::<
            Result<
                segments::Many<storage::Shared>,
                o3::mem::Rejected<
                    (
                        segments::Many<storage::Shared>,
                        segments::Many<storage::Shared>
                    ),
                    assembly::Error,
                >,
            >,
        >() == usize::BITS as usize
    );
    assert!(
        size_of::<
            Result<
                segments::Many<storage::Shared>,
                o3::mem::Rejected<
                    (
                        segments::Many<storage::Shared>,
                        segments::Item<storage::Shared>
                    ),
                    assembly::Error,
                >,
            >,
        >() == 9 * size_of::<usize>()
    );
};

#[test]
fn joins_without_allocation() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 4,
        chains: 2,
    })
    .expect("same-pool metadata");
    let left = try_many(&pool).expect("first chain");
    let right = try_many(&pool).expect("second chain");
    let pointers = left
        .as_chain()
        .segments()
        .chain(right.as_chain().segments())
        .map(|owner| owner.as_slice().as_ptr())
        .collect::<Vec<_>>();

    let (joined, allocs) = Tracker::count(|| segments::Many::try_from((left, right)));
    assert_eq!(allocs, 0);
    let joined = joined.expect("same-pool join");
    assert!(
        joined
            .as_chain()
            .segments()
            .map(|owner| owner.as_slice().as_ptr())
            .eq(pointers)
    );
    drop(joined);
    assert!(pool.is_idle());
}

#[test]
fn keeps_insert_cause() {
    for front in [false, true] {
        let pool = segments::Pool::try_new(segments::Plan {
            segments: 2,
            chains: 1,
        })
        .expect("metadata fixture");
        let chain = try_many(&pool).expect("full chain");
        let item = try_item(b"last").expect("incoming owner");
        let pointers = chain
            .as_chain()
            .segments()
            .map(|owner| owner.as_slice().as_ptr())
            .chain([item.as_slice().as_ptr()])
            .collect::<Vec<_>>();
        let before = pool.available();
        let (result, hot) = Tracker::measure(|| {
            Tracker::fail_after(0, || {
                if front {
                    segments::Many::try_from((item, chain))
                        .map_err(|error| error.map(|(item, chain)| (chain, item)))
                } else {
                    segments::Many::try_from((chain, item))
                }
            })
        });
        assert_eq!(hot, [0; 4]);
        let rejected = result.expect_err("fresh metadata allocation must fail");
        let ((chain, item), error) = rejected.into_parts();
        assert_eq!(
            error,
            assembly::Error::Nodes {
                shortage: capacity::Shortage {
                    requested: NonZero::<usize>::MIN,
                    available: 0,
                },
                allocation: alloc::Error::Exhausted,
            }
        );
        assert_eq!(pool.available().segments, before.segments);
        assert_eq!(pool.available().chains, before.chains);
        assert!(
            chain
                .as_chain()
                .segments()
                .map(|owner| owner.as_slice())
                .eq([b"first".as_slice(), b"second"])
        );
        assert_eq!(item.as_slice(), b"last");
        let (result, allocs) = Tracker::count(|| {
            if front {
                segments::Many::try_from((item, chain))
                    .map_err(|error| error.map(|(item, chain)| (chain, item)))
            } else {
                segments::Many::try_from((chain, item))
            }
        });
        let chain = result.expect("fallback assembly");
        assert_eq!(allocs, 1);
        let order = if front { [2, 0, 1] } else { [0, 1, 2] };
        assert!(
            chain
                .as_chain()
                .segments()
                .map(|owner| owner.as_slice().as_ptr())
                .eq(order.map(|index| pointers[index]))
        );
        assert!(pool.is_idle());
    }
}

#[test]
fn inserts_without_alloc() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    })
    .expect("local assembly pool");
    let chain = try_many(&pool).expect("retained chain");
    let item = try_item(b"last").expect("incoming owner");
    let (local, hot) = Tracker::measure(|| segments::Many::try_from((chain, item)));
    local.expect("available metadata admits locally");
    assert_eq!(hot, [0; 4]);
}

#[test]
fn joins_with_one_alloc() {
    let values: [&[u8]; 3] = [b"ab", b"cde", b"fghi"];
    for free in [0, 3] {
        let first = (segments::Pool::try_new(segments::Plan {
            segments: 2 + free,
            chains: 1,
        }))
        .expect("target pool allocation");
        let second = (segments::Pool::try_new(segments::Plan {
            segments: 3,
            chains: 1,
        }))
        .expect("source pool allocation");
        {
            let mut target = (first.try_chain()).expect("target chain edge");
            let mut source = (second.try_chain()).expect("source chain edge");
            target.try_fill(&values[..2]).expect("fixture segments fit");
            source.try_fill(&values).expect("fixture segments fit");
            let mut target = match target.into_shape() {
                segments::Shape::Many(many) => Some(many),
                _ => None,
            }
            .expect("target contains two owners");
            let mut source = match source.into_shape() {
                segments::Shape::Many(many) => Some(many),
                _ => None,
            }
            .expect("source contains three owners");
            if free == 0 {
                let result = Tracker::fail_after(0, || segments::Many::try_from((target, source)));

                let rejected = result.expect_err("fallback allocation must fail");
                let ((left, right), error) = rejected.into_parts();
                assert!(matches!(
                    error,
                    segments::assembly::Error::Nodes {
                        shortage,
                        allocation: o3::alloc::Error::Exhausted,
                    } if shortage.requested.get() == 3 && shortage.available == 0
                ));
                assert_eq!(
                    left.as_chain()
                        .try_bytes()
                        .expect("chain bytes must allocate"),
                    values[..2].concat()
                );
                assert_eq!(
                    right
                        .as_chain()
                        .try_bytes()
                        .expect("chain bytes must allocate"),
                    values.concat()
                );
                target = left;
                source = right;
            }
            let (result, hot) = Tracker::measure(|| segments::Many::try_from((target, source)));
            let joined = (result).expect("segment transition accepted");

            assert_eq!(hot[0], usize::from(free == 0));
            assert_eq!(hot[2], 0);
            assert_eq!(
                joined
                    .as_chain()
                    .try_bytes()
                    .expect("chain bytes must allocate"),
                [values[..2].concat(), values.concat()].concat()
            );
            assert!(
                joined
                    .as_chain()
                    .segments()
                    .zip(values[..2].iter().chain(values.iter()))
                    .all(|(owner, value)| { owner.as_slice().as_ptr() == value.as_ptr() })
            );
        }
        assert!(first.is_idle());
        assert!(second.is_idle());
    }
}

#[test]
fn checks_wrapped_ranges() {
    let created = queue::Fifo::try_with(queue::Capacity::new(Count::new(32), Count::new(3)));
    let mut source = created.expect("queue storage");
    examine(&source, b"");
    for bytes in [b"abc".as_slice(), b"de", b"fghij"] {
        let owner = storage::Shared::try_copy(bytes).expect("segment owner");
        source
            .try_push(owner)
            .as_ref()
            .expect("required fixture outcome");
    }
    examine(&source, b"abcdefghij");
    source
        .try_consume(3)
        .as_ref()
        .expect("required fixture outcome");
    let owner = storage::Shared::try_copy(b"klm").expect("wrapped owner");
    source
        .try_push(owner)
        .as_ref()
        .expect("required fixture outcome");
    examine(&source, b"defghijklm");
    source
        .try_consume(3)
        .as_ref()
        .expect("required fixture outcome");
    examine(&source, b"ghijklm");
    source
        .try_consume(7)
        .as_ref()
        .expect("required fixture outcome");
    examine(&source, b"");
}

#[test]
fn keeps_source_borrow() {
    let created = queue::Fifo::try_with(queue::Capacity::new(Count::new(3), Count::new(1)));
    let mut source = created.expect("queue storage");
    let owner = storage::Shared::try_copy(b"abc").expect("segment owner");
    let address = owner.as_slice().as_ptr();
    source
        .try_push(owner)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(first(&source), Ok(Some(b"abc")));
    let result = first(&source);
    assert!(matches!(result, Ok(Some(bytes)) if bytes.as_ptr() == address));
    let mut cursor = source.cursor();
    assert!(cursor.seek(usize::MAX).is_err());
    let selected = queue::Array::<_, 3>::try_new(&cursor, 0).expect("cursor remains at start");
    let mut bytes = [0; 3];
    selected.copy(&mut bytes);
    assert_eq!(bytes, *b"abc");
}

#[test]
fn returns_rejected_owner() {
    let created = queue::Fifo::try_with(queue::Capacity::new(Count::new(8), Count::new(0)));
    let mut source = created.expect("empty queue storage");
    source
        .try_push(storage::Shared::new())
        .as_ref()
        .expect("required fixture outcome");
    let owner = storage::Shared::try_copy(b"abc").expect("rejected owner");
    let address = owner.as_slice().as_ptr();
    let result = source.try_push(owner);
    assert!(result.is_err(), "zero segment capacity");
    let Err(rejected) = result else { return };
    let (owner, error) = rejected.into_parts();
    assert_eq!(error, queue::Error::Segments { capacity: 0 });
    assert_eq!(owner.as_slice(), b"abc");
    assert_eq!(owner.as_slice().as_ptr(), address);
    assert_eq!(source.len(), 0);
}

#[test]
fn consumes_admitted_prefix() {
    for amount in 0..=10 {
        let capacity = queue::Capacity::new(Count::new(32), Count::new(3));
        let mut source = queue::Fifo::try_with(capacity).expect("queue storage");
        for bytes in [b"abc".as_slice(), b"de", b"fghij"] {
            let owner = storage::Shared::try_copy(bytes).expect("initial owner");
            source
                .try_push(owner)
                .as_ref()
                .expect("required fixture outcome");
        }
        source.try_consume(3).expect("release front slot");
        let owner = storage::Shared::try_copy(b"klm").expect("wrapped owner");
        source
            .try_push(owner)
            .as_ref()
            .expect("required fixture outcome");
        let result = Drain::try_new(&mut source, 11);
        assert!(result.is_err(), "oversized prefix");
        let Err(rejected) = result else { return };
        let (required, room) = rejected.into_parts();
        assert_eq!((required.get(), room), (11, 10));
        {
            let pending = Drain::try_new(&mut source, amount).expect("bounded prefix");
            assert_eq!(pending.as_ref().len(), 10);
        }
        examine(&source, b"defghijklm");

        let pending = Drain::try_new(&mut source, amount).expect("unchanged admission");
        let ((), activity) = Tracker::measure(|| pending.commit());
        assert_eq!(activity[0], 0);
        assert_eq!(activity[1], 0);
        let released = [2, 7, 10].into_iter().filter(|&end| end <= amount).count();
        assert_eq!(activity[2], released);
        assert_eq!(source.segments(), 3 - released);
        examine(&source, &b"defghijklm"[amount..]);
        source.try_consume(10 - amount).expect("remaining suffix");
        assert_eq!(source.segments(), 0);
        let owner = storage::Shared::try_copy(b"next").expect("reused owner");
        source
            .try_push(owner)
            .as_ref()
            .expect("required fixture outcome");
        examine(&source, b"next");
    }
}

fn check<const N: usize>(source: &queue::Fifo<storage::Shared>, expected: &[u8]) {
    for offset in 0..=expected.len() + 1 {
        let selected = queue::Array::<_, N>::try_new(&source.cursor(), offset);
        match selected {
            Ok(mut selected) => {
                assert!(offset + N <= expected.len());
                let mut copied = [0; N];
                let ((), activity) = Tracker::measure(|| {
                    if let Some(bytes) = selected.contiguous() {
                        assert_eq!(bytes.as_slice(), &expected[offset..offset + N]);
                    }
                    selected.copy(&mut copied);
                });
                assert_eq!(copied.as_slice(), &expected[offset..offset + N]);
                assert_eq!(activity, [0; 4]);
            }
            Err(bounds) => {
                assert!(offset + N > expected.len());
                assert_eq!(
                    bounds,
                    queue::Bounds {
                        offset,
                        amount: N,
                        floor: 0,
                        ceiling: expected.len(),
                    }
                );
            }
        }
    }
    let overflow = queue::Array::<_, N>::try_new(&source.cursor(), usize::MAX);
    assert!(
        matches!(overflow, Err(queue::Bounds { offset: usize::MAX, amount, floor: 0, ceiling }) if amount == N && ceiling == expected.len())
    );
}

fn examine(source: &queue::Fifo<storage::Shared>, expected: &[u8]) {
    check::<0>(source, expected);
    check::<1>(source, expected);
    check::<3>(source, expected);
    check::<5>(source, expected);
    check::<9>(source, expected);
    check::<24>(source, expected);
}

fn first(source: &queue::Fifo<storage::Shared>) -> Result<Option<&[u8; 3]>, queue::Bounds> {
    queue::Array::try_new(&source.cursor(), 0).map(|mut array| array.contiguous())
}

#[test]
fn preserves_empty_owner() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 0,
        chains: 0,
    })
    .expect("empty metadata");
    let mut chain = pool.scoped();
    let mut fifo = queue::Fifo::try_with(queue::Capacity::new(Count::EMPTY, Count::EMPTY))
        .expect("empty queue");
    let mut owner = storage::Shared::try_copy(b"x").expect("retained backing");
    let resident = owner.resident();
    Slice::try_with(&mut owner, 0..0, Store::restrict).expect("retain empty range");
    let pointer = owner.as_slice().as_ptr();
    let (result, counts) = Tracker::counts(|| {
        let owner = segments::Item::new(owner)
            .break_value()
            .expect("empty owner remains unadmitted");
        let owner = match chain.try_push(owner).expect("empty admission") {
            segments::Push::Discarded(owner) => Some(owner),
            _ => None,
        }
        .expect("empty owner contributes no segment");
        assert_eq!(
            (owner.as_slice().as_ptr(), owner.resident()),
            (pointer, resident)
        );
        fifo.try_push(owner)
    });
    result
        .as_ref()
        .expect("empty owner bypasses both zero capacities");
    assert_eq!(counts, [0, 1]);
    assert!(chain.is_empty());
    assert!(fifo.is_empty());
}

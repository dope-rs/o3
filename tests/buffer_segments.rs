pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::mem;

use common::{
    alloc::system::Tracker,
    buffer::{Bytes as _, Fill as _},
};
use o3::{
    buffer::{
        self, Residency as _,
        hold::ranges,
        raw::Scatter as _,
        resident,
        segments::{self, queue},
        storage,
    },
    depot::capacity::Count,
};

#[test]
fn snapshots_retain_owners() {
    let values: [&[u8]; 3] = [b"ab", b"cde", b"fghi"];
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    }))
    .expect("the segmented cursor fixture must allocate");
    for count in 0..=values.len() {
        {
            let mut chain = (pool.try_chain()).expect("the chain edge must be available");
            chain
                .try_fill(&values[..count])
                .expect("fixture segments fit");
            let ((), activity) = Tracker::measure(|| {
                let mut cursor = chain.segments();
                for position in 0..=count {
                    let mut snapshot = cursor.clone();
                    assert_eq!(snapshot.len(), count - position);
                    for expected in &values[position..count] {
                        let owner =
                            (snapshot.next()).expect("the snapshot retains its remaining owners");
                        assert_eq!(owner.as_slice(), *expected);
                        assert_eq!(owner.as_slice().as_ptr(), expected.as_ptr());
                    }
                    assert!(snapshot.next().is_none());
                    assert_eq!(cursor.len(), count - position);
                    assert_eq!(
                        cursor.next().map(|owner| owner.as_slice()),
                        values[..count].get(position).copied()
                    );
                }
            });
            assert_eq!(activity, [0; 4]);
        }
        assert!(pool.is_idle());
    }
}

#[test]
fn retries_range_retention() {
    for (spans, expected) in [
        (&[(1, 3)][..], &b"ab"[..]),
        (&[(1, 3), (5, 8)][..], &b"abcde"[..]),
    ] {
        let retained = {
            let source =
                (storage::Shared::try_copy(b"_ab__cde_")).expect("range source allocation");
            let plan = (ranges::Set::<_, u16>::try_new(&source, spans, expected.len()))
                .expect("complete source ranges");
            let (failed, hot) = Tracker::measure(|| Tracker::fail_after(0, || plan.try_retain()));
            assert!(matches!(
                failed,
                Err(ranges::retain::Error::Metadata(o3::alloc::Error::Exhausted))
            ));
            assert_eq!(hot, [0; 4]);
            assert_eq!(plan.len(), expected.len());
            assert_eq!(plan.count().into_raw() as usize, spans.len());

            let (result, hot) = Tracker::measure(|| plan.try_retain());
            let retained = (result).expect("the unchanged plan must retry successfully");
            assert_eq!(hot[0], 1);
            assert_eq!(hot[2], 0);
            assert!(retained.count() == plan.count());
            assert_eq!(retained.len(), plan.len());
            assert_eq!(retained.resident(), source.resident() * spans.len());
            assert!(
                plan.iter()
                    .zip(retained.segments())
                    .all(|(view, owner)| { view.as_slice().as_ptr() == owner.as_slice().as_ptr() })
            );
            retained
        };
        assert_eq!(
            retained.try_bytes().expect("chain bytes must allocate"),
            expected
        );
    }
}

#[test]
fn drains_every_boundary() {
    let values: [&[u8]; 3] = [b"ab", b"cde", b"fghi"];
    let whole = values.concat();
    for count in 0..=values.len() {
        let len = values[..count]
            .iter()
            .map(|value| value.len())
            .sum::<usize>();
        for amount in 0..=len {
            let pool = (segments::Pool::try_new(segments::Plan {
                segments: 3,
                chains: 1,
            }))
            .expect("pool allocation");
            let mut chain = (pool.try_chain()).expect("chain edge");
            chain
                .try_fill(&values[..count])
                .expect("fixture segments fit");
            assert!(segments::Drain::try_new(&mut chain, len + 1).is_err());
            assert_eq!(
                chain.try_bytes().expect("chain bytes must allocate"),
                &whole[..len]
            );
            let ((), hot) = Tracker::measure(|| {
                let drain =
                    (segments::Drain::try_new(&mut chain, amount)).expect("validated byte prefix");
                assert_eq!(drain.as_ref().len(), len);
                drain.commit();
            });
            assert_eq!(hot, [0; 4]);
            assert_eq!(
                chain.try_bytes().expect("chain bytes must allocate"),
                &whole[amount..len]
            );
            let left = chain.count().into_raw() as usize;
            assert_eq!(pool.available().segments + left, 3);
            assert_eq!(pool.available().chains + usize::from(left > 1), 1);
            let ((), hot) = Tracker::measure(|| {
                let drain = (segments::Drain::try_new(&mut chain, len - amount))
                    .expect("remaining byte prefix");
                drain.commit();
            });
            assert_eq!(hot, [0; 4]);
            assert!(chain.is_empty());
            assert!(pool.is_idle());
        }
    }
    assert_eq!(
        mem::size_of::<segments::Drain<'_, segments::Chain<resident::Shared<'_>>>>(),
        16
    );
    assert_eq!(
        mem::size_of::<Option<segments::Chain<resident::Shared<'_>>>>(),
        16
    );
}

#[test]
fn transfers_every_boundary() {
    let values: [&[u8]; 3] = [b"ab", b"cde", b"fghi"];
    let whole = values.concat();
    for same in [true, false] {
        for present in 0..=2 {
            for amount in 0..=whole.len() {
                let first = (segments::Pool::try_new(segments::Plan {
                    segments: 12,
                    chains: 6,
                }))
                .expect("source pool allocation");
                let second = (segments::Pool::try_new(segments::Plan {
                    segments: 12,
                    chains: 6,
                }))
                .expect("target pool allocation");
                {
                    let mut source = (first.try_chain()).expect("source chain edge");
                    let target = if same { &first } else { &second };
                    let mut target = (target.try_chain()).expect("target chain edge");
                    for value in values {
                        let owner = (storage::Shared::try_copy(value))
                            .expect("retained payload allocation");
                        source
                            .try_push(resident::Shared::from(owner))
                            .as_ref()
                            .expect("required fixture outcome");
                    }
                    target
                        .try_fill(&values[..present])
                        .expect("fixture segments fit");
                    let ranges: Vec<_> = source
                        .segments()
                        .chain(target.segments())
                        .map(|owner| {
                            let bytes = owner.as_slice();
                            let start = bytes.as_ptr() as usize;
                            start..start + bytes.len()
                        })
                        .collect();
                    let segments::Error::Boundary(cause) = source
                        .try_move_to(&mut target, whole.len() + 1)
                        .expect_err("oversized prefix retains exact bounds")
                    else {
                        panic!("oversized prefix retains exact bounds");
                    };
                    let (required, room) = cause.into_parts();
                    assert_eq!(required.get(), whole.len() + 1);
                    assert_eq!(room, whole.len());
                    assert_eq!(
                        source.try_bytes().expect("chain bytes must allocate"),
                        whole
                    );
                    assert_eq!(
                        target.try_bytes().expect("chain bytes must allocate"),
                        values[..present].concat()
                    );
                    let mut wanted = values[..present].concat();
                    wanted.extend_from_slice(&whole[..amount]);
                    let (result, hot) =
                        Tracker::measure(|| source.try_move_to(&mut target, amount));
                    result.as_ref().expect("required fixture outcome");
                    assert_eq!(hot, [0; 4]);
                    assert_eq!(
                        source.try_bytes().expect("chain bytes must allocate"),
                        &whole[amount..]
                    );
                    assert_eq!(
                        target.try_bytes().expect("chain bytes must allocate"),
                        wanted
                    );
                    assert_eq!(
                        source.resident(),
                        source.segments().map(|owner| owner.resident()).sum()
                    );
                    assert_eq!(
                        target.resident(),
                        target.segments().map(|owner| owner.resident()).sum()
                    );
                    assert!(source.segments().chain(target.segments()).all(|owner| {
                        ranges
                            .iter()
                            .any(|range| range.contains(&(owner.as_slice().as_ptr() as usize)))
                    }));
                    let live = (source.count().into_raw() + target.count().into_raw()) as usize;
                    assert_eq!(
                        first.available().segments + second.available().segments + live,
                        24
                    );
                }
                assert!(first.is_idle());
                assert!(second.is_idle());
            }
        }
    }
}

#[test]
fn chain_reuses_metadata() {
    let first = (storage::Shared::try_copy(b"first")).expect("first chain backing must allocate");
    let first = resident::Shared::from(first);
    let second =
        (storage::Shared::try_copy(b"second")).expect("second chain backing must allocate");
    let second = resident::Shared::from(second);
    let pointers = [first.as_slice().as_ptr(), second.as_slice().as_ptr()];
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the two-segment chain pool must allocate");
    let mut chain = (pool.try_chain()).expect("chain edge must fit");
    let ((), allocs) = Tracker::count(|| {
        chain
            .try_push(first)
            .as_ref()
            .expect("required fixture outcome");
        chain
            .try_push(second)
            .as_ref()
            .expect("required fixture outcome");
    });

    assert_eq!(allocs, 0);
    assert!(chain.count() == segments::Count::TWO);
    let mut owners = chain.segments();
    let first = (owners.next()).expect("chain keeps first segment");
    let second = (owners.next()).expect("chain keeps second segment");
    assert_eq!(first.as_slice().as_ptr(), pointers[0]);
    assert_eq!(second.as_slice().as_ptr(), pointers[1]);
    assert!(owners.next().is_none());
}

#[test]
fn scopes_preserve_owners() {
    let first = *b"first";
    let last = *b"last";
    let result = segments::Pool::<resident::Shared<'_>>::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    });
    let pool = result.expect("required fixture outcome");
    let result = storage::Shared::try_copy(b"plain");
    let plain = result.expect("required fixture outcome");
    let pointers = [first.as_ptr(), plain.as_slice().as_ptr(), last.as_ptr()];
    let mut source = pool.scoped();
    source
        .try_push(resident::Shared::from(first.as_slice()))
        .as_ref()
        .expect("required fixture outcome");
    let held = (plain.try_clone()).expect("retained edge must fit");
    source
        .try_push(resident::Shared::from(held))
        .as_ref()
        .expect("required fixture outcome");
    source
        .try_push(resident::Shared::from(last.as_slice()))
        .as_ref()
        .expect("required fixture outcome");
    let resident: usize = source.segments().map(|owner| owner.resident()).sum();
    let expected = [first.as_slice(), last.as_slice()];
    let mut visited = 0;
    let (source, hot) = Tracker::measure(|| {
        for owner in source.segments() {
            match owner.scope() {
                resident::Scope::Independent => {}
                resident::Scope::Bound(owner) => {
                    assert!(
                        visited < expected.len(),
                        "each borrowed slot is visited once"
                    );
                    assert_eq!(owner.as_slice(), expected[visited]);
                    visited += 1;
                }
            }
        }
        source
    });
    assert_eq!(hot, [0; 4]);
    assert_eq!(visited, 2);
    assert_eq!(source.segments().len(), 3);
    assert_eq!(source.len(), 14);
    assert_eq!(
        source
            .segments()
            .map(|owner| owner.resident())
            .sum::<usize>(),
        resident
    );
    assert!(source.segments().map(|owner| owner.as_slice()).eq([
        first.as_slice(),
        b"plain",
        last.as_slice(),
    ]));
    assert!(
        source
            .segments()
            .zip(pointers)
            .all(|(owner, pointer)| owner.as_slice().as_ptr() == pointer)
    );
}

#[test]
fn shares_cursor_contract() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the two-owner cursor pool must allocate");
    let mut chain = (pool.try_chain()).expect("metadata edge must fit");
    let first =
        (storage::Shared::try_copy(b"abc")).expect("the first chain cursor owner must allocate");
    let second =
        (storage::Shared::try_copy(b"def")).expect("the second chain cursor owner must allocate");
    chain
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    chain
        .try_push(second)
        .as_ref()
        .expect("required fixture outcome");
    let cursor = (buffer::raw::Chunks::cursor(&chain, 2)).expect("chain offset two must exist");
    let mut ccur = cursor;
    assert_eq!(
        ccur.take_bytes(4)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"c".as_slice(), b"def".as_slice()]
    );

    let cap = queue::Capacity::new(Count::new(6), Count::new(2));
    let mut queue = (queue::Fifo::try_with(cap)).expect("the two-owner cursor queue must allocate");
    let first =
        (storage::Shared::try_copy(b"abc")).expect("the first queue cursor owner must allocate");
    let second =
        (storage::Shared::try_copy(b"def")).expect("the second queue cursor owner must allocate");
    queue
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    queue
        .try_push(second)
        .as_ref()
        .expect("required fixture outcome");
    let slice = (queue.get(1..5)).expect("queue slice one through five must exist");
    let cursor = (buffer::raw::Chunks::cursor(&slice, 1)).expect("slice offset one must exist");
    let mut qcur = cursor;
    assert_eq!(
        qcur.take_bytes(3)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"c".as_slice(), b"de".as_slice()]
    );
    let cursor = (buffer::raw::Chunks::cursor(&queue, queue.len()))
        .expect("queue end must be a valid cursor");
    let end = cursor;
    assert_eq!(end.remaining(), 0);
}

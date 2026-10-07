pub mod common;

use std::{array, num::NonZero};

use common::{
    alloc::system::Tracker,
    buffer::{try_item, try_many},
};
use o3::{
    buffer::{self, resident, segments, storage},
    depot::capacity,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn moves_full_prefix() {
    for prefix in 0..=2 {
        let pool = segments::Pool::try_new(segments::Plan {
            segments: 2 + prefix,
            chains: 1 + usize::from(prefix == 2),
        })
        .expect("exact metadata");
        let mut source: segments::Chain<_> = try_many(&pool).expect("two source owners").into();
        let pointers: [_; 2] =
            array::from_fn(|index| source.segments().nth(index).expect("source owner").as_ptr());
        let mut target = pool.try_chain().expect("target edge");
        for _ in 0..prefix {
            target
                .try_insert(try_item(b"prefix").expect("prefix owner"))
                .as_ref()
                .expect("prefix metadata");
        }
        let (result, activity) = Tracker::measure(|| source.try_move_to(&mut target, source.len()));
        result.expect("whole prefix transfer");
        assert_eq!(activity, [0; 4]);
        assert!(source.is_empty());
        assert_eq!(pool.available().chains, usize::from(prefix == 2));
        let (taken, activity) = Tracker::measure(|| target.try_take(target.len()));
        let taken = taken.expect("whole prefix extraction");
        assert_eq!(activity, [0; 4]);
        assert!(target.is_empty());
        assert!(
            taken
                .segments()
                .skip(prefix)
                .map(|owner| owner.as_ptr())
                .eq(pointers)
        );
        drop(taken);
        assert!(pool.is_idle());
    }
}

#[test]
fn keeps_prefix_failure() {
    for shared in [false, true] {
        let origin = segments::Pool::try_new(segments::Plan {
            segments: 2,
            chains: 0,
        })
        .expect("source pool");
        let other = segments::Pool::try_new(segments::Plan {
            segments: 1,
            chains: 0,
        })
        .expect("target pool");
        let mut source = origin.try_chain().expect("source edge");
        let mut target = if shared { &origin } else { &other }
            .try_chain()
            .expect("target edge");
        source
            .try_insert(try_item(b"source").expect("source owner"))
            .as_ref()
            .expect("source metadata");
        target
            .try_insert(try_item(b"target").expect("target owner"))
            .as_ref()
            .expect("target metadata");
        let pointers = [
            source.contiguous().expect("source bytes").as_ptr(),
            target.contiguous().expect("target bytes").as_ptr(),
        ];
        let shortage = capacity::Shortage {
            requested: NonZero::<usize>::MIN,
            available: 0,
        };
        let error = if shared {
            segments::Error::ChainCapacity(shortage)
        } else {
            segments::Error::SegmentCapacity(shortage)
        };
        let (result, activity) = Tracker::measure(|| source.try_move_to(&mut target, source.len()));
        assert_eq!(result, Err(error));
        assert_eq!(activity, [0; 4]);
        assert_eq!(source.contiguous(), Some(b"source".as_slice()));
        assert_eq!(target.contiguous(), Some(b"target".as_slice()));
        assert_eq!(
            source.contiguous().expect("source bytes").as_ptr(),
            pointers[0]
        );
        assert_eq!(
            target.contiguous().expect("target bytes").as_ptr(),
            pointers[1]
        );
    }
}

#[test]
fn flattens_with_budget() {
    let source = segments::Many::try_from((
        try_item(b"abc").expect("first owner"),
        try_item(b"def").expect("second owner"),
    ))
    .expect("source metadata");
    let pointer = source.front().as_slice().as_ptr();
    let rejected = Tracker::fail_after(0, || segments::Flatten::try_new(source))
        .err()
        .expect("output allocation rejected");
    assert_eq!(
        rejected.error(),
        &storage::AllocError::Exhausted(o3::alloc::Exhausted)
    );
    let (source, _) = rejected.into_parts();
    assert_eq!(source.front().as_slice().as_ptr(), pointer);

    let (copy, allocs) = Tracker::count(|| segments::Flatten::try_new(source));
    assert_eq!(allocs, 1);
    let mut copy = copy.expect("output allocation");
    let (output, allocs) = Tracker::count(|| {
        for budget in [1, 3, 1] {
            copy = copy
                .step(NonZero::new(budget).expect("positive budget"))
                .continue_value()
                .expect("copy budget exhausted");
        }
        copy.step(NonZero::new(2).expect("positive budget"))
            .break_value()
            .expect("complete output")
    });
    assert_eq!(allocs, 0);
    assert_eq!(output.as_slice(), b"abcdef");
}

#[test]
fn insertion_returns_item() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 0,
    })
    .expect("pool allocation");
    let mut chain = pool.try_chain().expect("metadata edge");
    chain
        .try_insert(try_item(b"first").expect("nonempty fixture owner"))
        .as_ref()
        .expect("metadata admits the retained item");
    let second = try_item(b"second").expect("nonempty fixture owner");
    let pointer = second.as_ref().as_slice().as_ptr();
    let (result, activity) = Tracker::measure(|| chain.try_insert(second));
    assert_eq!(activity, [0; 4]);

    assert!(result.is_err(), "descriptor rejection");
    let Err(error) = result else { return };
    let (second, error) = error.into_parts();
    assert!(
        error
            == segments::Error::ChainCapacity(capacity::Shortage {
                requested: NonZero::<usize>::MIN,
                available: 0,
            })
    );
    assert!(second.count() == segments::Count::ONE);
    assert_eq!(second.as_ref().as_slice().as_ptr(), pointer);
    assert_eq!(chain.contiguous(), Some(b"first".as_slice()));
    assert_eq!(pool.available().segments, 1);
}

#[test]
fn extension_returns_many() {
    for plan in [
        segments::Plan {
            segments: 1,
            chains: 0,
        },
        segments::Plan {
            segments: 2,
            chains: 0,
        },
    ] {
        let origin = segments::Pool::try_new(segments::Plan {
            segments: 2,
            chains: 1,
        })
        .expect("source pool allocation");
        let source = try_many(&origin).expect("two-owner fixture");
        let pointer = source.front().as_slice().as_ptr();
        let pool = segments::Pool::try_new(plan).expect("target pool allocation");
        let mut target = pool.try_chain().expect("target edge");
        let (result, activity) = Tracker::measure(|| target.try_extend(source));
        assert_eq!(activity, [0; 4]);

        assert!(result.is_err(), "target capacity rejection");
        let Err(error) = result else { return };
        let (source, error) = error.into_parts();
        match plan.segments {
            1 => assert!(
                error
                    == segments::Error::SegmentCapacity(capacity::Shortage {
                        requested: NonZero::new(2).expect("positive request"),
                        available: 1,
                    })
            ),
            _ => assert!(
                error
                    == segments::Error::ChainCapacity(capacity::Shortage {
                        requested: NonZero::<usize>::MIN,
                        available: 0,
                    })
            ),
        }
        assert!(source.as_chain().count() == segments::Count::TWO);
        assert_eq!(source.front().as_slice().as_ptr(), pointer);
        assert!(
            source
                .as_chain()
                .segments()
                .map(|owner| owner.as_slice())
                .eq([b"first".as_slice(), b"second"])
        );
        assert!(target.is_empty());
        assert_eq!(origin.available().segments, 0);
        assert_eq!(origin.available().chains, 0);
        assert_eq!(pool.available().segments, plan.segments);
        assert_eq!(pool.available().chains, plan.chains);
    }
}

#[test]
fn extension_reuses_nodes() {
    for shared in [false, true] {
        let origin = segments::Pool::try_new(segments::Plan {
            segments: 3,
            chains: 1,
        })
        .expect("source pool allocation");
        let pool = segments::Pool::try_new(segments::Plan {
            segments: 3,
            chains: 1,
        })
        .expect("target pool allocation");
        let source = try_many(&origin).expect("two-owner fixture");
        let pointers: [_; 2] = array::from_fn(|index| {
            source
                .as_chain()
                .segments()
                .nth(index)
                .expect("source owner")
                .as_slice()
                .as_ptr()
        });
        let pool = if shared { &origin } else { &pool };
        let mut target = pool.try_chain().expect("target edge");
        target
            .try_insert(try_item(b"prefix").expect("nonempty fixture owner"))
            .as_ref()
            .expect("metadata admits the retained item");
        let (result, activity) = Tracker::measure(|| target.try_extend(source));
        result.as_ref().expect("required fixture outcome");
        assert_eq!(activity, [0; 4]);
        assert!(
            target
                .segments()
                .skip(1)
                .zip(pointers)
                .all(|(owner, pointer)| owner.as_slice().as_ptr() == pointer)
        );
        let ((), activity) = Tracker::measure(|| {
            let front = target.front_entry().expect("pooled front").take();
            target
                .try_insert(front)
                .as_ref()
                .expect("metadata admits the retained item");
        });
        assert_eq!(activity, [0; 4]);
        assert!(target.count() == segments::Count::new(3));
        assert!(target.segments().map(|owner| owner.as_slice()).eq([
            b"first".as_slice(),
            b"second",
            b"prefix"
        ]));
        assert_eq!(pool.available().segments, 0);
    }
}

#[test]
fn join_returns_owners() {
    let first = segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    })
    .expect("left pool allocation");
    let second = segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    })
    .expect("right pool allocation");
    let left = try_many(&first).expect("two-owner fixture");
    let right = try_many(&second).expect("two-owner fixture");
    let pointers = [
        left.front().as_slice().as_ptr(),
        right.front().as_slice().as_ptr(),
    ];
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || segments::Many::try_from((left, right))));
    assert_eq!(activity, [0; 4]);

    assert!(result.is_err(), "exact metadata rejection");
    let Err(error) = result else { return };
    let ((left, right), error) = error.into_parts();
    assert!(
        matches!(error, segments::assembly::Error::Nodes { shortage, .. }
        if shortage.requested.get() == 2 && shortage.available == 0)
    );
    for (source, pointer) in [left, right].iter().zip(pointers) {
        assert!(source.as_chain().count() == segments::Count::TWO);
        assert_eq!(source.front().as_slice().as_ptr(), pointer);
    }
}

#[test]
fn discard_preserves_node() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    })
    .expect("metadata pool");
    {
        let source = try_many(&pool).expect("two-owner fixture");
        let pointer = source
            .as_chain()
            .segments()
            .nth(1)
            .expect("last owner")
            .as_slice()
            .as_ptr();
        let logical = source.front().len();
        let resident = buffer::Residency::resident(source.front());
        let ((rest, charge), activity) = Tracker::counts(|| source.discard());
        assert_eq!(activity, [0, 1]);
        assert_eq!(charge.logical_bytes, logical);
        assert_eq!(charge.resident_bytes, resident);
        assert_eq!(charge.owners, 1);
        let last = match rest {
            segments::Nonempty::One(last) => Some(last),
            segments::Nonempty::Many(_) => None,
        }
        .expect("one retained pooled owner");
        assert_eq!(last.as_ref().as_slice(), b"second");
        assert_eq!(last.as_ref().as_slice().as_ptr(), pointer);
        assert_eq!(pool.available().segments, 1);
        assert_eq!(pool.available().chains, 1);
    }
    assert_eq!(pool.available().segments, 2);
    assert_eq!(pool.available().chains, 1);
}

#[test]
fn discard_preserves_many() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    })
    .expect("metadata pool");
    {
        let mut chain = pool.try_chain().expect("metadata edge");
        for bytes in [b"first".as_slice(), b"second", b"third"] {
            chain
                .try_insert(try_item(bytes).expect("nonempty fixture owner"))
                .as_ref()
                .expect("metadata admits the retained item");
        }
        let source = match chain.into_shape() {
            segments::Shape::Many(source) => Some(source),
            _ => None,
        }
        .expect("three owners");
        let logical = source.front().len();
        let resident = buffer::Residency::resident(source.front());
        let ((rest, charge), activity) = Tracker::counts(|| source.discard());
        assert_eq!(activity, [0, 1]);
        assert_eq!(charge.logical_bytes, logical);
        assert_eq!(charge.resident_bytes, resident);
        assert_eq!(charge.owners, 1);
        let rest = match rest {
            segments::Nonempty::Many(rest) => Some(rest),
            segments::Nonempty::One(_) => None,
        }
        .expect("two retained owners");
        assert_eq!(rest.len().get(), b"secondthird".len());
        assert_eq!(
            buffer::Residency::resident(rest.as_chain()),
            rest.as_chain()
                .segments()
                .map(buffer::Residency::resident)
                .sum::<usize>()
        );
        assert!(rest.as_chain().count() == segments::Count::TWO);
        assert!(
            rest.as_chain()
                .segments()
                .map(|owner| owner.as_slice())
                .eq([b"second".as_slice(), b"third",])
        );
        assert_eq!(pool.available().segments, 1);
        assert_eq!(pool.available().chains, 0);
    }
    assert_eq!(pool.available().segments, 3);
    assert_eq!(pool.available().chains, 1);
}

#[test]
fn transfers_complete_front() {
    let first = *b"head";
    let last = *b"tail";
    let pool = (segments::Pool::<resident::Shared<'_>>::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the two-segment pool must allocate");
    let mut owners = (pool.try_chain()).expect("fresh pool must admit a chain owner");
    assert!(
        owners
            .try_push(resident::Shared::from(first.as_slice()))
            .is_ok()
    );
    assert!(
        owners
            .try_push(resident::Shared::from(last.as_slice()))
            .is_ok()
    );

    let segments::Shape::Many(owners) = owners.into_shape() else {
        panic!("two owners");
    };
    let chunk = segments::Nonempty::Many(owners);
    assert!(!chunk.is_single());
    let pointer = chunk.front().as_slice().as_ptr();
    let (result, count) = Tracker::count(|| chunk.try_pop(usize::MAX));
    let (head, rest) = result
        .ok()
        .expect("a complete front must move without retaining");
    assert_eq!(count, 0);
    assert_eq!(head.as_slice(), first);
    assert_eq!(head.as_slice().as_ptr(), pointer);
    assert_eq!(rest.contiguous(), Some(last.as_slice()));
    assert_eq!(rest.segments().len(), 1);
    assert_eq!(rest.len(), last.len());
    assert!(pool.is_idle());
}

#[test]
fn preserves_partial_front() {
    let mut first = Vec::with_capacity(32);
    first.extend_from_slice(b"head");
    let capacity = first.capacity();
    let pointer = first.as_ptr();
    let first = (storage::Shared::try_from(&mut first)).expect("the vector owner must allocate");
    let last = *b"tail";
    let pool = (segments::Pool::<resident::Shared<'_>>::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the two-segment pool must allocate");
    let mut owners = (pool.try_chain()).expect("fresh pool must admit a chain owner");
    assert!(owners.try_push(resident::Shared::from(first)).is_ok());
    assert!(
        owners
            .try_push(resident::Shared::from(last.as_slice()))
            .is_ok()
    );

    let segments::Shape::Many(owners) = owners.into_shape() else {
        panic!("two owners");
    };
    let chunk = segments::Nonempty::Many(owners);
    let (result, activity) = Tracker::measure(|| chunk.try_pop(2));
    let (head, rest) = result
        .ok()
        .expect("the partial front must preserve its original chain");
    assert_eq!(activity, [0; 4]);
    assert_eq!(rest.len(), 6);
    assert_eq!(rest.segments().len(), 2);
    let mut segments = rest.segments();
    let first = (segments.next()).expect("the remaining front must be present");
    assert_eq!(first, b"ad");
    assert_eq!(first.as_ptr(), pointer.wrapping_add(2));
    assert_eq!(segments.next(), Some(last.as_slice()));
    assert_eq!(segments.next(), None);
    drop(segments);
    let ((), activity) = Tracker::measure(|| drop(rest));
    assert_eq!(activity, [0; 4]);
    assert!(pool.is_idle());
    assert_eq!(head.as_slice(), b"he");
    assert_eq!(head.as_slice().as_ptr(), pointer);
    let ((), freed) = Tracker::measure(|| drop(head));
    assert_eq!(freed, [0, 0, 2, capacity + 4 * size_of::<usize>()]);
}

#[test]
fn preserves_remainder() {
    let data = *b"head";
    for limit in [0, 2, data.len(), usize::MAX] {
        let chunk = segments::Nonempty::One(segments::Item::from(&data));
        let (result, activity) = Tracker::measure(|| chunk.try_pop(limit));
        let (head, rest) = result
            .ok()
            .expect("a borrowed prefix needs no retention capacity");
        assert_eq!(activity, [0; 4]);
        let split = limit.min(data.len());
        assert_eq!(head.as_slice(), &data[..split]);
        if split != 0 {
            assert_eq!(head.as_slice().as_ptr(), data.as_ptr());
        }
        assert_eq!(rest.contiguous(), Some(&data[split..]));
        assert_eq!(rest.segments().len(), usize::from(split < data.len()));
        assert_eq!(rest.is_empty(), split == data.len());
        if split < data.len() {
            let segments::Shape::One(rest) = rest else {
                panic!("single suffix");
            };
            assert_eq!(rest.as_ref().as_slice().as_ptr(), data[split..].as_ptr());
        }
    }
}

use std::pin::Pin;

use o3::{
    base::cell::region,
    buffer::{
        hold,
        raw::Scatter as _,
        resident,
        segments::{self, queue, scatter::Source as _},
        storage::{self, Shared},
        write::consume::Consume as _,
    },
    depot::capacity::Count,
};

#[test]
fn borrows_prefixes() {
    let mut queue = queue::Fifo::try_with(queue::Capacity::new(Count::new(4), Count::new(2)))
        .expect("source queue");
    assert!(queue.prefix(usize::MAX).is_empty());
    for bytes in [b"ab", b"cd"] {
        queue
            .try_push(resident::Shared::from(bytes.as_slice()))
            .as_ref()
            .expect("source owner");
    }
    queue.try_consume(2).expect("release first slot");
    queue
        .try_push(resident::Shared::from(b"ef".as_slice()))
        .as_ref()
        .expect("reuse wrapped slot");
    assert!(queue.prefix(3).fragments().flatten().eq(b"cde"));
    assert!(queue.prefix(usize::MAX).fragments().flatten().eq(b"cdef"));
}

#[test]
fn projects_suffixes() {
    let mut queue = queue::Fifo::try_with(queue::Capacity::new(Count::new(9), Count::new(3)))
        .expect("source queue");
    let retain = hold::Retention::<resident::Shared<'_>>::default();
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    })
    .expect("metadata");
    let empty = queue
        .get(0..0)
        .expect("empty range")
        .try_bound(&pool, &retain, hold::Policy::compact())
        .expect("empty projection needs no copy authority");
    assert_eq!(empty.count(), segments::Count::EMPTY);
    assert!(empty.is_empty());
    drop(empty);

    for bytes in [b"abc", b"def", b"ghi"] {
        queue
            .try_push(resident::Shared::from(bytes.as_slice()))
            .as_ref()
            .expect("source owner");
    }
    for (wrapped, bytes) in [(false, b"abcdefghi"), (true, b"defghijkl")] {
        if wrapped {
            queue.try_consume(3).expect("release first slot");
            queue
                .try_push(resident::Shared::from(b"jkl".as_slice()))
                .as_ref()
                .expect("reuse wrapped slot");
        }
        let suffix = queue.get(4..9).expect("middle-owner suffix");
        let short = segments::Pool::try_new(segments::Plan {
            segments: 1,
            chains: 0,
        })
        .expect("insufficient metadata");
        let error = suffix
            .try_bound(&short, &retain, hold::Policy::compact())
            .expect_err("segment shortage precedes descriptor and copy failures");
        assert!(matches!(
            error,
            queue::RetainError::Segments(segments::Error::SegmentCapacity(cause))
                if cause.requested.get() == 2 && cause.available == 1
        ));
        assert!(short.is_idle());
        let error = suffix
            .try_bound(&pool, &retain, hold::Policy::compact())
            .expect_err("nonempty copy requires authority");
        assert_eq!(
            error,
            queue::RetainError::Copy(hold::CopyError::Authority(resident::BindError::Unbound))
        );
        assert!(pool.is_idle());

        for (range, count) in [(0..9, 3), (3..9, 2), (4..9, 2), (9..9, 0)] {
            let held = queue
                .get(range.clone())
                .expect("suffix range")
                .try_bound(&pool, &retain, hold::Policy::share())
                .expect("metadata and source remain reusable");
            assert_eq!(held.count().into_raw(), count);
            assert!(
                held.segments()
                    .flat_map(|part| part.as_slice())
                    .eq(&bytes[range])
            );
        }
    }
}

#[test]
fn projects_mixed_owners() {
    let first = Shared::try_copy(b"ab").expect("small owner");
    let large = Shared::try_copy(&[b'x'; 64]).expect("large owner");
    let first_at = first.as_slice().as_ptr();
    let large_at = large.as_slice().as_ptr();
    let mut queue = queue::Fifo::try_with(queue::Capacity::new(Count::new(66), Count::new(2)))
        .expect("source queue");
    queue.try_push(first).as_ref().expect("first owner");
    queue.try_push(large).as_ref().expect("large owner");
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    })
    .expect("projected metadata");
    let retain = hold::Retention::<Shared>::default();
    let held = queue
        .get(0..4)
        .expect("mixed range")
        .try_bound(&pool, &retain, hold::Policy::new(1, 2))
        .expect("share small owner and copy large projection");

    assert!(
        held.chunks()
            .take_bytes(usize::MAX)
            .flat_map(|chunk| chunk.bytes())
            .eq(b"abxx")
    );
    assert_eq!(held.resident(), 4);
    {
        let mut owners = held.segments();
        assert_eq!(
            owners.next().expect("shared owner").as_slice().as_ptr(),
            first_at
        );
        assert_ne!(
            owners.next().expect("copied owner").as_slice().as_ptr(),
            large_at
        );
    }
    drop(held);
    assert!(pool.is_idle());
}

#[test]
fn restores_projection() {
    let mut domain = resident::Domain::new();
    region::Token::scope_pinned(Pin::new(&mut domain), |token, domain| {
        let budget = domain
            .get_mut()
            .try_budget(2, token.proof())
            .expect("two copied bytes");
        let mut retain = hold::Retention::<resident::Shared<'_>>::default();
        retain.try_bind(budget).expect("copy authority");
        let mut queue = queue::Fifo::try_with(queue::Capacity::new(Count::new(74), Count::new(3)))
            .expect("source queue");
        for owner in [
            resident::Shared::from(b"abcdefgh".as_slice()),
            resident::Shared::from(b"ij".as_slice()),
            resident::Shared::from(Shared::try_copy(&[b'k'; 64]).expect("owned source")),
        ] {
            queue.try_push(owner).as_ref().expect("source owner");
        }
        let pool = segments::Pool::try_new(segments::Plan {
            segments: 3,
            chains: 1,
        })
        .expect("projected metadata");
        let slice = queue.get(0..12).expect("one share and two copies");
        let error = slice
            .try_bound(&pool, &retain, hold::Policy::new(8, 2))
            .expect_err("second copy exceeds the unchanged budget");
        assert!(matches!(
            error,
            queue::RetainError::Copy(hold::CopyError::Resident(
                resident::AllocError::Budget(cause)
            )) if cause.into_parts().0.get() == 2 && cause.into_parts().1 == 0
        ));
        assert_eq!(budget.resident(), 0);
        assert!(pool.is_idle());
        let mut bytes = [0; 12];
        queue::Array::try_new(&queue.cursor(), 0)
            .expect("unchanged source range")
            .copy(&mut bytes);
        assert_eq!(&bytes, b"abcdefghijkk");

        let shared = slice.try_retain(&pool).expect("all metadata was returned");
        assert_eq!(shared.count().into_raw(), 3);
        assert_eq!(pool.available().segments, 0);
        assert_eq!(pool.available().chains, 0);
        drop(shared);
        assert!(pool.is_idle());
    });
}

#[test]
fn recycles_owner_slots() {
    let cap = queue::Capacity::new(Count::new(3), Count::new(2));
    let mut queue =
        queue::Fifo::<Shared>::try_with(cap).expect("the two-owner recycle queue must allocate");
    let first = (storage::Shared::try_copy(&[1])).expect("the first recycle owner must allocate");
    let second = (storage::Shared::try_copy(&[2])).expect("the second recycle owner must allocate");
    let third =
        (storage::Shared::try_copy(&[3])).expect("the rejected recycle owner must allocate");
    let address = third.as_slice().as_ptr();
    queue
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    queue
        .try_push(second)
        .as_ref()
        .expect("required fixture outcome");
    let rejected = queue.try_push(third).expect_err("the segment ring is full");
    let (third, error) = rejected.into_parts();
    assert_eq!(error, queue::Error::Segments { capacity: 2 });
    assert_eq!(third.as_slice().as_ptr(), address);

    let () = (queue.try_consume(1)).expect("the queued one-byte prefix must be consumable");
    queue
        .try_push(third)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(queue.segments(), 2);
    assert_eq!(queue.spare_len(), 1);
}

#[test]
fn rejects_byte_overflow() {
    let cap = queue::Capacity::new(Count::new(2), Count::new(1));
    let mut queue = (queue::Fifo::try_with(cap)).expect("the byte-overflow queue must allocate");
    let (first, rejected) = (
        (storage::Shared::try_copy(b"ab")).expect("the byte-overflow owners must allocate"),
        (storage::Shared::try_copy(b"c")).expect("the byte-overflow owners must allocate"),
    );
    let address = rejected.as_slice().as_ptr();
    queue
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    queue
        .try_push(Shared::new())
        .as_ref()
        .expect("empty owner bypasses both exhausted capacities");
    let rejected = queue
        .try_push(rejected)
        .expect_err("byte overflow precedes the exhausted segment ring");
    let (rejected, error) = rejected.into_parts();
    assert!(
        matches!(error, queue::Error::Bytes { requested, available: 0 } if requested.get() == 1)
    );
    assert_eq!(rejected.as_slice(), b"c");
    assert_eq!(rejected.as_slice().as_ptr(), address);
    assert_eq!(queue.segments(), 1);
    assert_eq!(queue.len(), 2);
}

#[test]
fn keeps_owner_boundaries() {
    let first =
        (storage::Shared::try_copy(b"first")).expect("the first boundary owner must allocate");
    let alloc = first.as_slice().as_ptr();
    let cap = queue::Capacity::new(Count::new(11), Count::new(2));
    let mut queue = (queue::Fifo::try_with(cap)).expect("the boundary queue must allocate");
    queue
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    let second =
        (storage::Shared::try_copy(b"second")).expect("the second boundary owner must allocate");
    queue
        .try_push(second)
        .as_ref()
        .expect("required fixture outcome");

    let contig = queue
        .get(1..4)
        .expect("range lies within the first segment")
        .contiguous()
        .expect("range has one owner");
    assert_eq!(contig, b"irs");
    assert_eq!(contig.as_ptr(), alloc.wrapping_add(1));
    assert!(
        queue
            .get(4..7)
            .expect("range spans the segment boundary")
            .contiguous()
            .is_none()
    );
    assert_eq!(
        queue
            .get(4..7)
            .expect("range spans the segment boundary")
            .fragments()
            .collect::<Vec<_>>(),
        [b"t".as_slice(), b"se".as_slice()]
    );
}

#[test]
fn advances_cursor() {
    let cap = queue::Capacity::new(Count::new(9), Count::new(3));
    let mut queue = (queue::Fifo::try_with(cap)).expect("the cursor queue must allocate");
    let first = (storage::Shared::try_copy(b"abc")).expect("the first cursor owner must allocate");
    let second =
        (storage::Shared::try_copy(b"def")).expect("the second cursor owner must allocate");
    queue
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    queue
        .try_push(second)
        .as_ref()
        .expect("required fixture outcome");
    let third = storage::Shared::try_copy(b"ghi").expect("third cursor owner");
    queue
        .try_push(third)
        .as_ref()
        .expect("third cursor segment");

    let mut cursor = queue.cursor();
    assert_eq!(cursor.contiguous(4..7), None);
    assert_eq!(cursor.contiguous(1..3), Some(b"bc".as_slice()));
    let mut copied = [0; 4];
    queue::Array::try_new(&cursor, 2)
        .expect("required fixture outcome")
        .copy(&mut copied);
    assert_eq!(&copied, b"cdef");

    assert_eq!(cursor.contiguous(3..3), Some(b"".as_slice()));
    let rejected = queue::Array::<_, 1>::try_new(&cursor, 2);
    assert!(rejected.is_err());
    let Err(bounds) = rejected else { return };
    assert_eq!(bounds.offset, 2);
    assert_eq!(bounds.amount, 1);
    assert_eq!(bounds.floor, 3);
    assert_eq!(bounds.ceiling, 9);

    assert_eq!(cursor.contiguous(9..9), Some(b"".as_slice()));
    queue::Array::<_, 0>::try_new(&cursor, 9)
        .expect("required fixture outcome")
        .copy(&mut []);
    assert!(cursor.seek(5).is_err());
}

#[test]
fn restores_failed_slots() {
    let first = (storage::Shared::try_copy(b"a")).expect("the first rollback owner must allocate");
    let base = first.as_slice().as_ptr();
    let cap = queue::Capacity::new(Count::new(2), Count::new(2));
    let mut queue = (queue::Fifo::try_with(cap)).expect("the rollback queue must allocate");
    queue
        .try_push(first)
        .as_ref()
        .expect("required fixture outcome");
    let second =
        (storage::Shared::try_copy(b"b")).expect("the second rollback owner must allocate");
    queue
        .try_push(second)
        .as_ref()
        .expect("required fixture outcome");

    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 1,
        chains: 0,
    }))
    .expect("the one-segment rollback pool must allocate");
    assert!(matches!(
        queue
            .get(0..2)
            .expect("range spans both queued segments")
            .try_retain(&pool),
        Err(segments::Error::SegmentCapacity(
            o3::depot::capacity::Shortage {
                requested,
                available: 1,
            }
        )) if requested.get() == 2
    ));
    assert_eq!(pool.available().segments, 1);

    let held = queue
        .get(0..1)
        .expect("range lies within the first segment")
        .try_retain(&pool);
    let held = (held).expect("segment transition accepted");

    assert_eq!(held.contiguous(), Some(b"a".as_slice()));
    assert_eq!(
        held.contiguous()
            .expect("retained range has one owner")
            .as_ptr(),
        base
    );
}

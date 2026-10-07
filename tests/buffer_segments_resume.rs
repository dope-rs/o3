pub mod common;

use std::{hint, mem::size_of};

use common::{
    alloc::system::Tracker,
    buffer::{Fill as _, scatter::Slices},
};
use o3::{
    buffer::{
        self,
        raw::Scatter as _,
        segments::{
            self, queue,
            scatter::{self, Source as _},
        },
        storage,
    },
    depot::capacity::Count,
};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn resume_partial_moves() {
    let cases: &[&[&[u8]]] = &[&[b"abcdefghi"], &[b"ab", b"cde", b"fghi"]];
    for parts in cases {
        let pool = segments::Pool::try_new(segments::Plan {
            segments: 3,
            chains: 1,
        })
        .expect("pool");
        let mut chain = pool.try_chain().expect("chain");
        chain.try_fill(parts).expect("owners");
        let expected = b"abcdefghi";
        let mut position = scatter::raw::Position::new();
        let mut chain = Some(chain);
        let ((), activity) = Tracker::measure(|| {
            for offset in [0, 1, 2, 3, 5, 7, 8, 4, 0, 8] {
                let owner = chain.take().expect("retained owner");
                let offset = hint::black_box(offset);
                // SAFETY: every call retains this unchanged chain; every offset is
                // within its nine bytes, including repeated and backward offsets.
                let mut cursor = unsafe { position.resume(&owner, offset) };
                assert_eq!(cursor.remaining(), expected.len() - offset);
                let mut used = offset;
                for chunk in cursor.take_bytes(expected.len()) {
                    let bytes = chunk.bytes();
                    assert_eq!(bytes, &expected[used..used + bytes.len()]);
                    used += bytes.len();
                }
                assert_eq!(used, expected.len());
                chain = Some(owner);
            }
        });
        assert_eq!(activity, [0; 4]);
    }
}

#[test]
fn keeps_borrowed_layout() {
    type Cursor = <Slices<'static> as buffer::raw::Chunks>::Cursor<'static>;
    assert_eq!(
        size_of::<scatter::Take<'static, 'static, Cursor>>(),
        2 * size_of::<usize>()
    );
}

#[test]
fn crosses_segments() {
    let parts: [&[u8]; 5] = [b"", b"abc", b"", b"defgh", b"ij"];
    let source = (Slices::try_new(&parts)).expect("the fixture length must fit usize");
    let mut cursor = source.chunks();

    assert_eq!(
        cursor
            .take_bytes(4)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"abc".as_slice(), b"d".as_slice()]
    );
    assert_eq!(
        cursor
            .take_bytes(3)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"efg".as_slice()]
    );
    assert_eq!(
        cursor
            .take_bytes(8)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"h".as_slice(), b"ij".as_slice()]
    );
    assert!(cursor.take_bytes(1).next().is_none());
}

#[test]
fn keeps_unread_bytes() {
    let parts: [&[u8]; 2] = [b"abc", b"def"];
    let source = (Slices::try_new(&parts)).expect("the fixture length must fit usize");
    let mut cursor = source.chunks();

    assert!(cursor.take_bytes(0).next().is_none());
    {
        let mut first = cursor.take_bytes(5);
        assert_eq!(first.next().map(|chunk| chunk.bytes()), Some(&b"abc"[..]));
    }
    assert_eq!(
        cursor
            .take_bytes(2)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"de".as_slice()]
    );
    assert_eq!(
        cursor
            .take_bytes(8)
            .map(|chunk| chunk.bytes())
            .collect::<Vec<_>>(),
        [b"f".as_slice()]
    );
}

#[test]
fn bounds_static_chunks() {
    let parts: [&[u8]; 2] = [b"abc", b"def"];
    let source = Slices::try_new(&parts).expect("bounded source");
    let mut cursor = buffer::raw::Chunks::cursor(&source, 1).expect("cursor");
    {
        let mut take = cursor.take_bytes(4);
        let first = take.next_chunk::<3>().expect("first chunk");
        let second = take.next_chunk::<3>().expect("second chunk");
        assert_eq!(first.len().get(), first.bytes().len());
        assert_eq!(second.len().get(), second.bytes().len());
        assert_eq!((first.bytes(), second.bytes()), (&b"bc"[..], &b"de"[..]));
        assert_eq!(first.bytes().as_ptr(), parts[0][1..].as_ptr());
        assert!(take.next_chunk::<3>().is_none());
    }
    assert_eq!(
        cursor
            .take_bytes(1)
            .next_chunk::<1>()
            .expect("tail")
            .bytes(),
        b"f"
    );
    assert!(buffer::raw::Scatter::seek(&b"abc"[..], 4).is_none());
}

#[test]
fn matches_flat_reference() {
    let parts: [&[u8]; 7] = [b"", b"abc", b"d", b"", b"efgh", b"ij", b""];
    let cases: &[(&[&[u8]], &[u8])] = &[(&[], b""), (&[b"", b""], b""), (&parts, b"abcdefghij")];
    for (parts, expected) in cases {
        let source = Slices::try_new(parts).expect("the fixture length must fit usize");
        compare(&source, expected);
    }
}

#[test]
fn preserves_native_ranges() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 6,
        chains: 3,
    }))
    .expect("both native chains must allocate their metadata");
    let mut owned = (pool.try_chain()).expect("the owned chain must retain its pool");
    let mut scoped = pool.scoped();
    let capacity = queue::Capacity::new(Count::new(6), Count::new(2));
    let mut queue =
        (queue::Fifo::try_with(capacity)).expect("the native queue must allocate its metadata");
    owned
        .try_push(storage::Shared::new())
        .as_ref()
        .expect("required fixture outcome");
    scoped
        .try_push(storage::Shared::new())
        .as_ref()
        .expect("required fixture outcome");
    compare(&owned, b"");
    compare(&scoped, b"");
    compare(&queue, b"");
    for (index, bytes) in [b"abc", b"def"].into_iter().enumerate() {
        let first = (storage::Shared::try_copy(bytes)).expect("the physical owner must allocate");
        let second = first
            .try_clone()
            .expect("the scoped source must retain its owner");
        let third = first
            .try_clone()
            .expect("the queued source must retain its owner");
        owned
            .try_push(first)
            .as_ref()
            .expect("required fixture outcome");
        scoped
            .try_push(second)
            .as_ref()
            .expect("required fixture outcome");
        queue
            .try_push(third)
            .as_ref()
            .expect("required fixture outcome");
        let expected = &b"abcdef"[..3 * (index + 1)];
        compare(&owned, expected);
        compare(&scoped, expected);
        compare(&queue, expected);
    }
    let prefix = (owned.try_take(4)).expect("segment transition accepted");

    compare(&prefix, b"abcd");
    compare(&owned, b"ef");
    assert!(scoped.pop_segment().is_some());
    compare(&scoped, b"def");
}

fn compare(source: &impl scatter::Source, expected: &[u8]) {
    let (_, activity) = Tracker::activity(|| {
        assert_eq!(buffer::raw::Chunks::len(source), expected.len());
        assert_eq!(source.suffix::<0>(), Some([]));
        assert_eq!(source.suffix::<3>(), expected.last_chunk::<3>().copied());
        assert_eq!(source.suffix::<11>(), expected.last_chunk::<11>().copied());
        let first = source.chunks().take_bytes(1).next();
        assert_eq!(first.map(|chunk| chunk.bytes()), expected.get(..1));
        for start in 0..=expected.len() {
            for limit in [1, 2, 5, usize::MAX] {
                let raw = (buffer::raw::Chunks::cursor(source, start))
                    .expect("each bounded native suffix must exist");
                let mut cursor = raw;
                assert_eq!(cursor.remaining(), expected.len() - start);
                let mut output = [0; 10];
                let mut end = 0;
                while cursor.remaining() != 0 {
                    let before = cursor.remaining();
                    for chunk in cursor.take_bytes(limit) {
                        let next = end + chunk.len().get();
                        output[end..next].copy_from_slice(chunk.bytes());
                        end = next;
                    }
                    assert!(cursor.remaining() < before);
                }
                assert!(cursor.take_bytes(1).next().is_none());
                assert_eq!(&output[..end], &expected[start..]);
            }
        }
        assert!(buffer::raw::Chunks::cursor(source, expected.len() + 1).is_none());
    });
    assert_eq!(activity.allocations, 0);
    assert_eq!(activity.deallocations, 0);
}

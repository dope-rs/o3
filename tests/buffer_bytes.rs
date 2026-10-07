pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{hint, num::NonZero};

use common::{alloc::system::Tracker, buffer::domain::Capacity};
use o3::{
    alloc,
    base::num::bounded::U32,
    buffer::{
        self,
        bytes::{Prefix, Source, TryRetain, bounded, bounded::raw::Input, window::reserve},
        hold::ranges,
        resident::{self, copy},
        segments,
        storage::{self, Shared, provided},
        write::consume::Consume as _,
    },
};

#[test]
fn retains_copy_charge() {
    let observed = Capacity(4)
        .try_bytes(*b"body", |source, budget| {
            let count: U32<0, { bounded::MAX as u32 }> = (&source).into();
            assert_eq!(count.get(), 4);
            {
                let charge = budget.try_charge(1).expect("one byte charge");
                let result = resident::Shared::try_copy(&source, &budget);
                assert!(matches!(result,
                    Err(copy::Error::Budget(error))
                    if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(3), 3)
                ));
                assert_eq!(charge.capacity(), 1);
            }
            let (mut selected, pointer) = {
                let (copied, activity) =
                    Tracker::counts(|| resident::Shared::try_copy(&source, &budget));
                let copied = copied.expect("four byte budget");
                assert_eq!(activity, [1, 0]);
                assert_eq!(copied.as_slice(), b"body");
                assert_eq!(budget.resident(), 4);
                let pointer = copied.as_ptr();
                let (selected, hot) =
                    Tracker::measure(|| copied.try_clone().expect("retained edge").project(1..3));
                let selected = selected.expect("buffer transition accepted");

                assert_eq!(hot, [0; 4]);
                (selected, pointer)
            };
            assert_eq!(selected.as_slice(), b"od");
            assert_eq!(selected.as_ptr(), pointer.wrapping_add(1));
            assert_eq!(budget.resident(), 4);
            selected.try_consume(1).expect("one byte prefix");
            assert_eq!(selected.as_slice(), b"d");
            assert_eq!(budget.resident(), 4);
            selected.try_consume(1).expect("last byte");
            assert!(selected.is_empty());
            assert_eq!(budget.resident(), 0);
        })
        .expect("provided byte fixture");
    assert_eq!(observed, (0, Some(*b"body")));
}

#[test]
fn copy_failure() {
    let observed = Capacity(4)
        .try_bytes(*b"body", |source, budget| {
            let result = Tracker::fail_after(0, || {
                hint::black_box(resident::Shared::try_copy(&source, &budget))
            });
            assert!(matches!(
                result,
                Err(copy::Error::Allocation(alloc::Exhausted))
            ));
            assert_eq!(budget.resident(), 0);
            assert_eq!(source.as_ref(), b"body");
            {
                let copied = resident::Shared::try_copy(&source, &budget)
                    .expect("retry uses restored credit");
                assert_eq!(copied.as_slice(), b"body");
                assert_eq!(budget.resident(), 4);
            }
            assert_eq!(budget.resident(), 0);
        })
        .expect("provided byte fixture");
    assert_eq!(observed, (0, Some(*b"body")));
}

#[test]
fn empty_copy() {
    let observed = Capacity(0)
        .try_bytes(*b"", |source, budget| {
            let (copied, activity) =
                Tracker::measure(|| resident::Shared::try_copy(&source, &budget));
            let copied = copied.expect("empty copy needs no credit");
            assert!(copied.is_empty());
            assert_eq!(activity, [0; 4]);
            assert_eq!(budget.resident(), 0);
        })
        .expect("provided byte fixture");
    assert_eq!(observed, (0, Some(*b"")));
}

#[test]
fn prefix_length_bound() {
    let prefix = Prefix::from(b"bytes");
    let exact: U32<0, 5> = prefix.len();
    let wider: U32<0, 31> = prefix.len();
    assert_eq!(exact.get(), 5);
    assert_eq!(wider.get(), 5);
    let clipped = Prefix::<3>::new(prefix.as_slice());
    assert_eq!(clipped.len::<3>().get(), 3);
    let empty = Prefix::<0>::new(prefix.as_slice());
    assert_eq!(empty.len::<0>().get(), 0);
}

#[test]
fn splits_admitted_prefix() {
    // SAFETY: this unchanged literal has exactly the admitted five bytes.
    const PREFIX: Prefix<'static, 5> = Prefix::from_raw(unsafe { Input::new(b"bytes") });
    let (first, rest) = PREFIX.split_first().expect("first byte");
    let (head, tail) = rest.split_array::<2>().expect("two byte header");
    let (last, empty) = tail.split_at(2).expect("complete tail");
    assert_eq!(*first, b'b');
    assert_eq!(head, b"yt");
    assert_eq!(last.as_slice(), b"es");
    assert_eq!(last.len::<5>().get(), 2);
    assert_eq!(
        last.as_slice().as_ptr(),
        PREFIX.as_slice().as_ptr().wrapping_add(3)
    );
    assert_eq!(empty.len::<5>().get(), 0);
    assert!(empty.split_first().is_none());
    assert!(tail.split_array::<3>().is_none());
    assert!(PREFIX.split_at(6).is_none());
}

#[test]
fn admitted_owner() {
    let bytes = Vec::from(&b"bounded"[..]);
    let pointer = bytes.as_ptr();
    let (owner, activity) = Tracker::measure(|| bounded::Value::try_new(bytes));
    let owner = owner
        .map_err(|rejected| rejected.into_parts().1)
        .expect("the owned bytes fit the signed count");
    assert_eq!(activity, [0; 4]);
    assert_eq!(owner.as_ref(), b"bounded");
    assert_eq!(owner.as_ref().as_ptr(), pointer);
}

#[test]
fn rejection_layout() {
    use std::mem::{align_of, size_of};

    assert_eq!(
        size_of::<Result<bounded::Value<Vec<u8>>, bounded::Rejected<Vec<u8>>>>(),
        size_of::<Result<bounded::Value<Vec<u8>>, Vec<u8>>>(),
    );
    assert_eq!(
        size_of::<Result<bounded::Value<&[u8]>, bounded::Rejected<&[u8]>>>(),
        size_of::<Result<bounded::Value<&[u8]>, &[u8]>>(),
    );
    assert_eq!(
        size_of::<
            Result<
                bounded::Value<storage::Shared<provided::Owner<'_>>>,
                bounded::Rejected<storage::Shared<provided::Owner<'_>>>,
            >,
        >(),
        size_of::<
            Result<
                bounded::Value<storage::Shared<provided::Owner<'_>>>,
                storage::Shared<provided::Owner<'_>>,
            >,
        >(),
    );
    assert_eq!(
        align_of::<bounded::Rejected<Vec<u8>>>(),
        align_of::<Vec<u8>>()
    );
}

#[test]
fn retain_costs_match() {
    let borrowed = b"borrowed".as_slice();
    let source = Source::<Shared>::Borrowed(borrowed);
    assert_eq!(source.as_ref(), borrowed);
    let (result, allocs) = Tracker::count(|| source.try_retain());
    let copied = result.expect("borrowed bytes must retain into owned storage");
    assert_eq!(allocs, 1);
    assert_eq!(copied.as_slice(), borrowed);
    assert_ne!(copied.as_slice().as_ptr(), borrowed.as_ptr());

    let shared =
        (storage::Shared::try_copy(b"shared")).expect("shared storage backing must allocate");
    let pointer = shared.as_slice().as_ptr();
    let (result, allocs) = Tracker::count(|| TryRetain::try_retain(shared));
    let retained = result.expect("shared storage must retain without allocation");
    assert_eq!(allocs, 0);
    assert_eq!(retained.as_slice().as_ptr(), pointer);

    let shared =
        (storage::Shared::try_copy(b"source")).expect("owned source backing must allocate");
    let pointer = shared.as_slice().as_ptr();
    let source = Source::Owned(shared);
    assert_eq!(source.as_ref(), b"source");
    let (result, allocs) = Tracker::count(|| source.try_retain());
    let retained = result.expect("owned source must retain without allocation");
    assert_eq!(allocs, 0);
    assert_eq!(retained.as_slice().as_ptr(), pointer);
    assert_eq!(retained.as_slice(), b"source");
}

#[test]
fn retains_scoped_view() {
    let retained = {
        let copied = Shared::try_copy(b"abcdef");
        let copied = copied.expect("scoped view fixture must be copyable");
        let source = resident::Shared::from(copied);
        let pointer = source.as_slice().as_ptr().wrapping_add(1);
        let retained = source
            .get(1..5)
            .expect("range lies within scoped bytes")
            .try_retain()
            .expect("asserted retained edge");
        assert_eq!(retained.as_slice().as_ptr(), pointer);
        retained
    };
    assert_eq!(retained.as_slice(), b"bcde");
}

#[test]
fn retains_owned_view() {
    let retained = {
        let copied = Shared::try_copy(b"abcdef");
        let source = copied.expect("owned view fixture must be copyable");
        let pointer = source.as_slice().as_ptr().wrapping_add(1);
        let view =
            (buffer::Cursor::try_get(&source, 1..5)).expect("the range lies within owned bytes");
        let retained = (view.try_retain()).expect("the owned view must be retainable");
        assert_eq!(retained.as_slice().as_ptr(), pointer);
        retained
    };
    assert_eq!(retained.as_slice(), b"bcde");
}

#[test]
fn validates_compact_ranges() {
    let source = b"_ab__cde_".as_slice();
    let spans = [(1, 3), (5, 8)];
    let ranges = ranges::Set::<_, u16>::try_new(source, &spans, 5);
    let set = ranges.expect("complete compact ranges must validate");
    let parts = set.iter().map(|view| view.as_slice()).collect::<Vec<_>>();

    assert_eq!(parts, [b"ab".as_slice(), b"cde".as_slice()]);
    assert_eq!(set.contiguous(), None);
    assert_eq!(set.len(), 5);
    assert!(set.count() == segments::Count::TWO);
    assert!(matches!(
        ranges::Set::<_, u16>::try_new(source, &[(1, 10)], 9),
        Err(ranges::Error::Bounds {
            index: 0,
            start: 1,
            end: 10,
            len: 9,
        })
    ));
}

#[test]
fn omits_empty_backing() {
    let copied = Shared::try_copy(b"abcdef");
    let copied = copied.expect("empty view fixture must be copyable");
    let source = resident::Shared::from(copied);
    let retained = source
        .get(2..2)
        .expect("empty range lies within scoped bytes")
        .try_retain()
        .expect("asserted retained edge");

    assert!(retained.is_empty());
    assert!(retained.backing().is_none());
}

#[test]
fn hot_paths_do_not_alloc() {
    let shared = Shared::try_copy(&[7; 4_096]).expect("hot-path shared backing must allocate once");
    let ((), allocs) = Tracker::count(|| {
        let view = shared
            .try_get(64..4_032)
            .expect("range retention must fit")
            .expect("range must belong to the shared backing");
        hint::black_box(view.as_slice());
        let sibling = shared.try_clone().expect("retained edge must fit");
        let view = sibling
            .project(64..4_032)
            .expect("buffer transition accepted");
        hint::black_box(view.as_slice());
    });
    assert_eq!(allocs, 0);
}

#[test]
fn keeps_spare_capacity() {
    let plan = (reserve::Plan::<17>::new(NonZero::<usize>::MIN.saturating_add(7)))
        .expect("the terminal and eight usable bytes must be representable");
    let mut ring = (reserve::Ring::try_new(plan)).expect("the reserved ring must allocate");
    reserve::Append::try_new(&mut ring, [&b"abcdefgh"[..]], usize::MAX)
        .map(reserve::Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    let error = reserve::Append::try_new(&mut ring, [&b"x"[..]], usize::MAX)
        .err()
        .expect("ordinary writes cannot consume the terminal reservation");
    assert_eq!(error.into_parts(), (NonZero::<usize>::MIN, 0));
    assert_eq!(ring.as_slices(), (&b"abcdefgh"[..], &[][..]));
    ring.finish(&[42; 17]);
    assert_eq!(ring.len(), 25);
    assert_eq!(
        ring.as_slices().0,
        [b"abcdefgh".as_slice(), &[42; 17]].concat()
    );
    ring.finish(&[99; 17]);
    assert_eq!(ring.len(), 25);
    let read = reserve::Prefix::new(&mut ring, usize::MAX);
    assert_eq!(read.len(), 25);
    read.commit();
    {
        (reserve::Append::try_new(&mut ring, [&[3; 25][..]], usize::MAX)
            .map(reserve::Append::commit))
        .as_ref()
        .expect("required fixture outcome");
    };
    assert_eq!(ring.len(), 25);
    ring.finish(&[99; 17]);
    assert_eq!(ring.as_slices().0, &[3; 25]);
}

#[test]
fn wraps_reserved_bytes() {
    let plan = (reserve::Plan::<17>::new(NonZero::<usize>::MIN.saturating_add(7)))
        .expect("the terminal and eight usable bytes must be representable");
    let mut ring = (reserve::Ring::try_new(plan)).expect("the reserved ring must allocate");
    reserve::Append::try_new(&mut ring, [&b"abcdefgh"[..]], 8)
        .map(reserve::Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    let read = reserve::Prefix::new(&mut ring, 7);
    assert_eq!(read.len(), 7);
    read.commit();
    reserve::Append::try_new(&mut ring, [&b"ijkl"[..], &b"mno"[..]], 8)
        .map(reserve::Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    ring.finish(&[42; 17]);
    let (first, second) = ring.as_slices();
    assert_eq!(first, [b"hijklmno".as_slice(), &[42; 10]].concat());
    assert_eq!(second, &[42; 7]);
    let read = reserve::Prefix::new(&mut ring, 9);
    assert_eq!(read.len(), 9);
    read.commit();
    assert_eq!(ring.len(), 16);
    let (first, second) = ring.as_slices();
    assert_eq!([first, second].concat(), &[42; 16]);
}

#[test]
fn admission_is_atomic() {
    let plan = (reserve::Plan::<17>::new(NonZero::<usize>::MIN.saturating_add(7)))
        .expect("the terminal and eight usable bytes must be representable");
    let mut ring = (reserve::Ring::try_new(plan)).expect("the reserved ring must allocate");
    let error = reserve::Append::try_new(&mut ring, [&b"abcde"[..], &b"fghi"[..]], 8)
        .err()
        .expect("every source must fit before the first write");
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(3), 3)
    );
    assert_eq!(ring.len(), 0);
    reserve::Append::try_new(&mut ring, [&b"abcdefgh"[..]], 8)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(ring.len(), 0);
    ring.finish(&[42; 17]);
    assert_eq!(ring.as_slices().0, &[42; 17]);
}

#[test]
fn rejects_overflow() {
    let logical = NonZero::<usize>::MAX;
    let result = reserve::Plan::<17>::new(logical);
    assert!(matches!(result, Err(error) if error == reserve::Error {
        capacity: logical,
        reserved: 17,
    }));
    let logical = NonZero::<usize>::MIN.saturating_add(isize::MAX as usize - 1);
    assert!(reserve::Plan::<17>::new(logical).is_err());
    let logical = NonZero::<usize>::MIN.saturating_add(isize::MAX as usize - 18);
    let plan = (reserve::Plan::<17>::new(logical))
        .expect("the exact allocation ceiling must be representable");
    assert_eq!(plan.capacity().get(), isize::MAX as usize);
}

#[test]
fn owns_selected_prefix() {
    let plan = (reserve::Plan::<17>::new(NonZero::<usize>::MIN.saturating_add(7)))
        .expect("the ring plan must fit");
    let mut ring = (reserve::Ring::try_new(plan)).expect("the ring must allocate");
    reserve::Append::try_new(&mut ring, [&b"abcdefgh"[..]], 8)
        .map(reserve::Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    {
        let mut read = reserve::Prefix::new(&mut ring, usize::MAX);
        assert_eq!(read.total(), 8);
        read.truncate(3);
        read.truncate(7);
        assert_eq!(read.len(), 3);
        assert_eq!(read.as_slices(), (&b"abc"[..], &[][..]));
    }
    assert_eq!(ring.len(), 8);
    let read = reserve::Prefix::new(&mut ring, 3);
    let ring = read.commit();
    assert_eq!(ring.as_slices(), (&b"defgh"[..], &[][..]));
    ring.finish(&[42; 17]);
    let read = reserve::Prefix::new(ring, usize::MAX);
    assert_eq!(read.len(), 22);
    let ring = read.commit();
    assert_eq!(ring.len(), 0);
    {
        (reserve::Append::try_new(ring, [&[99; 25][..]], usize::MAX).map(reserve::Append::commit))
            .as_ref()
            .expect("required fixture outcome");
    };
    assert_eq!(ring.as_slices(), (&[99; 25][..], &[][..]));
}

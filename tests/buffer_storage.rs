pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, mem, num::NonZero, pin};

use common::{
    alloc::system::Tracker,
    buffer::{
        domain::Capacity,
        provided::{Fixed, Item},
    },
};
use o3::{
    alloc,
    buffer::{
        self, Residency as _, resident,
        storage::{self, provided},
        write::{Sink as _, SpareExt as _},
    },
};

#[test]
fn keeps_vec_owner() {
    let mut data = Vec::with_capacity(32);
    data.extend_from_slice(b"payload");
    let capacity = data.capacity();
    let pointer = data.as_ptr();
    let (result, created) = Tracker::measure(|| storage::Shared::try_from(&mut data));
    let shared = (result).expect("the vector control block must allocate");
    assert_eq!(created, [1, 4 * size_of::<usize>(), 0, 0]);
    assert_eq!(shared.resident(), capacity);
    let (result, activity) = Tracker::measure(|| shared.try_clone());
    let plain = (result).expect("the plain edge must retain its vector");
    assert_eq!(activity, [0; 4]);
    let (result, activity) = Tracker::measure(|| plain.project(2..6));
    let plain = result.expect("the vector's initialized range must project");
    assert_eq!(activity, [0; 4]);
    assert_eq!(plain.as_ptr(), pointer.wrapping_add(2));
    assert_eq!(plain.resident(), capacity);
    let scoped = resident::Shared::from(shared);
    let (result, activity) = Tracker::measure(|| scoped.try_clone());
    let held = (result).expect("the scoped edge must retain the same vector");
    assert_eq!(activity, [0; 4]);
    assert_eq!(held.resident(), capacity);
    let ((), activity) = Tracker::measure(move || {
        assert_eq!(plain.as_slice(), &scoped.as_slice()[2..6]);
        drop(plain);
    });
    assert_eq!(activity, [0; 4]);
    let ((), freed) = Tracker::measure(move || {
        assert_eq!(held.as_slice(), b"payload");
        assert_eq!(held.as_slice().as_ptr(), pointer);
    });
    assert_eq!(freed, [0, 0, 2, capacity + created[1]]);
}

#[test]
fn retries_vec_admission() {
    let mut source = Vec::with_capacity(64);
    source.extend_from_slice(b"retained input");
    let pointer = source.as_ptr();
    let capacity = source.capacity();
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || storage::Shared::try_from(&mut source)));
    assert!(matches!(result, Err(alloc::Exhausted)));
    assert_eq!(activity, [0; 4]);
    assert_eq!(source.as_ptr(), pointer);
    assert_eq!(source.capacity(), capacity);
    assert_eq!(source, b"retained input");
    let owner = storage::Shared::try_from(&mut source).expect("the same input can retry");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(owner.as_slice(), b"retained input");
    assert_eq!(source.capacity(), 0);
}

#[test]
fn retries_text_admission() {
    let mut source = String::from("한글 input");
    let pointer = source.as_ptr();
    let capacity = source.capacity();
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || storage::Str::try_from(&mut source)));
    assert!(matches!(result, Err(alloc::Exhausted)));
    assert_eq!(activity, [0; 4]);
    assert_eq!(source.as_ptr(), pointer);
    assert_eq!(source.capacity(), capacity);
    assert_eq!(source, "한글 input");
    let owner = storage::Str::try_from(&mut source).expect("the valid text can retry");
    assert_eq!(owner.as_bytes().as_ptr(), pointer);
    assert_eq!(owner.as_str(), "한글 input");
    assert_eq!(source.capacity(), 0);
}

#[test]
fn detaches_empty_borrow() {
    let detached = {
        let input = *b"abcdef";
        let source = resident::Shared::from(&input[1..5]);
        let (result, activity) = Tracker::measure(|| source.try_detach());
        assert_eq!(activity, [0; 4]);
        let source = result.expect_err("a nonempty borrow keeps its lifetime");
        assert_eq!(source.as_slice(), b"bcde");
        assert_eq!(source.as_slice().as_ptr(), input[1..5].as_ptr());

        let result = source.project(2..2);
        let empty = result.expect("the empty range lies within the returned extent");
        let (result, activity) = Tracker::measure(|| empty.try_detach());
        assert_eq!(activity, [0; 4]);
        result.expect("an empty borrow detaches without copying")
    };
    assert!(detached.as_slice().is_empty());
}

#[test]
fn denial_keeps_capacity() {
    Capacity(6)
        .try_scope(|budget| {
            let (result, activity) =
                Tracker::measure(|| resident::Exact::try_copy(&budget, b"payload"));

            let error = match result {
                Err(resident::AllocError::Budget(error)) => Some(error),
                _ => None,
            }
            .expect("seven bytes must exceed the six-byte budget");
            let (need, spare) = error.into_parts();
            assert_eq!((need.get(), spare), (7, 6));
            assert_eq!(activity, [0; 4]);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn exhaustion_refunds() {
    Capacity(7)
        .try_scope(|budget| {
            let (result, activity) = Tracker::measure(|| {
                Tracker::fail_after(0, || resident::Exact::try_copy(&budget, b"payload"))
            });
            assert!(matches!(
                result,
                Err(resident::AllocError::Exhausted(alloc::Exhausted))
            ));
            assert_eq!(activity, [0; 4]);
            assert_eq!(budget.resident(), 0);
            let ((), activity) = Tracker::measure(|| {
                let exact = (resident::Exact::try_copy(&budget, b"payload"))
                    .expect("the refunded budget must admit the retry");
                assert_eq!(exact.as_slice(), b"payload");
                assert_eq!(budget.resident(), 7);
            });
            assert_eq!(activity, [1, 24 + 7, 1, 24 + 7]);
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn plain_copy_owns_once() {
    let (result, allocs) = Tracker::measure(|| storage::Exact::try_copy(b"payload"));
    let exact = (result).expect("the plain exact source must allocate");
    assert_eq!((allocs[0], allocs[2]), (1, 0));
    assert_eq!(exact.as_slice(), b"payload");
    let pointer = exact.as_slice().as_ptr();
    let (shared, activity) = Tracker::measure(|| exact.into_shared());
    assert_eq!(activity, [0; 4]);
    let (held, activity) = Tracker::measure(|| shared.try_clone().expect("asserted retained edge"));
    assert_eq!(activity, [0; 4]);
    let ((), activity) = Tracker::measure(move || {
        assert_eq!(shared.as_slice().as_ptr(), pointer);
        drop(shared);
    });
    assert_eq!(activity, [0; 4]);
    let ((), freed) = Tracker::measure(move || {
        assert_eq!(held.as_slice(), b"payload");
        assert_eq!(held.as_slice().as_ptr(), pointer);
        drop(held);
    });
    assert_eq!((freed[0], freed[2]), (0, 1));
    assert_eq!(freed[3], allocs[1]);
}

#[test]
fn plain_copy_oom() {
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || storage::Exact::try_copy(b"payload")));
    assert!(matches!(
        result,
        Err(storage::AllocError::Exhausted(alloc::Exhausted))
    ));
    assert_eq!(activity, [0; 4]);
}

#[test]
fn empty_copy_is_free() {
    let (result, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || storage::Exact::try_copy(&[])));
    let exact = (result).expect("empty copy must not ask the exhausted allocator");
    assert!(exact.as_slice().is_empty());
    assert_eq!(activity, [0; 4]);
    assert_eq!(size_of::<Option<storage::Exact>>(), size_of::<usize>() * 2);
}

#[test]
fn build_keeps_source() {
    let source = [17, 29];
    let (result, activity) = Tracker::measure(|| {
        storage::Exact::try_build(8, |out| {
            out.write_slice(b"partial")
                .as_ref()
                .expect("required fixture outcome");
            Err(source.as_slice())
        })
    });

    let error = match result {
        Err(storage::BuildError::Build(error)) => Some(error),
        _ => None,
    }
    .expect("the callback failure must reach its caller intact");
    assert_eq!(error, source);
    assert_eq!(error.as_ptr(), source.as_ptr());
    assert_eq!((activity[0], activity[2]), (1, 1));
    assert_eq!(activity[1], activity[3]);
}

#[test]
fn short_build_frees_owner() {
    let (result, activity) =
        Tracker::measure(|| storage::Exact::try_build(5, |out| out.write_slice(b"four")));

    let gap = match result {
        Err(storage::BuildError::Length(gap)) => Some(gap),
        _ => None,
    }
    .expect("underfill must return the exact missing span");
    let (expected, actual) = gap.into_parts();
    assert_eq!((expected.get(), actual), (5, 4));
    assert_eq!((activity[0], activity[2]), (1, 1));
    assert_eq!(activity[1], activity[3]);
}

#[test]
fn exhaustion_skips_build() {
    let mut invoked = false;
    let (result, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            storage::Exact::try_build(5, |out| {
                invoked = true;
                out.write_slice(b"exact")
            })
        })
    });
    assert!(!invoked, "allocation failure must precede the callback");
    assert!(matches!(
        result,
        Err(storage::BuildError::Exhausted(alloc::Exhausted))
    ));
    assert_eq!(activity, [0; 4]);
}

#[test]
fn build_transfers() {
    let (result, allocs) = Tracker::measure(|| {
        storage::Exact::try_build(5, |out| {
            b"exact".iter().try_for_each(|byte| out.write_byte(*byte))
        })
    });
    let exact = (result).expect("the complete writer must produce an exact owner");
    assert_eq!((allocs[0], allocs[2]), (1, 0));
    assert_eq!(exact.as_slice(), b"exact");
    assert_eq!(exact.resident(), 5);
    let pointer = exact.as_slice().as_ptr();
    let (shared, activity) = Tracker::measure(|| exact.into_shared());
    assert_eq!(activity, [0; 4]);
    let ((), freed) = Tracker::measure(move || assert_eq!(shared.as_slice().as_ptr(), pointer));
    assert_eq!((freed[0], freed[2]), (0, 1));
    assert_eq!(allocs[1], freed[3]);
}

#[test]
fn empty_build_calls() {
    let mut called = false;
    let (result, activity) = Tracker::measure(|| {
        Tracker::fail_after(0, || {
            storage::Exact::try_build(0, |out| {
                called = true;
                out.write_slice(&[])
            })
        })
    });
    let exact = (result).expect("empty construction must succeed without allocation");
    assert!(called);
    assert!(exact.as_slice().is_empty());
    assert_eq!(activity, [0; 4]);
}

#[test]
fn bind_keeps_edge() {
    let (result, allocs) = Tracker::measure(|| storage::Exact::try_copy(b"plain"));
    let plain = (result).expect("the plain exact owner must allocate");
    let pointer = plain.as_slice().as_ptr();
    let (result, count) = Tracker::counts(|| {
        let bound = resident::Exact::from_plain(plain);
        bound.try_clone().map(|held| (bound, held))
    });
    let (bound, held) = (result).expect("the exact owner must retain its backing");
    assert_eq!(count, [0, 0]);
    let ((), count) = Tracker::counts(move || assert_eq!(held.as_slice(), b"plain"));
    assert_eq!(count, [0, 0]);
    let ((), freed) =
        Tracker::measure(move || assert_eq!(bound.into_shared().as_slice().as_ptr(), pointer));
    assert_eq!((allocs[0], allocs[2], freed[0], freed[2]), (1, 0, 0, 1));
    assert_eq!(allocs[1], freed[3]);
}

#[test]
fn compact_retries() {
    Capacity(16)
        .try_scope(|budget| {
            let mut owner = (storage::Owned::try_allocate(&budget, 16))
                .expect("the initial owner must fit the budget");
            let mut spare = owner.spare();
            assert_eq!(spare.write(b"payload"), 7);
            spare.commit();
            let pointer = owner.as_ptr();
            let (result, activity) =
                Tracker::measure(|| Tracker::fail_after(0, || owner.try_compact()));
            assert!(matches!(result, Err(alloc::Exhausted)));
            assert_eq!(activity, [0; 4]);
            assert_eq!(owner.as_ptr(), pointer);
            assert_eq!(&owner[..], b"payload");
            assert_eq!((owner.capacity(), budget.resident()), (16, 16));
            let (result, resized) = Tracker::measure(|| owner.try_compact());
            result.as_ref().expect("required fixture outcome");
            assert_eq!((resized[0], resized[2]), (1, 1));
            assert_eq!(resized[3] - resized[1], 9);
            assert_eq!((owner.capacity(), budget.resident()), (7, 7));
            let pointer = owner.as_ptr();
            let (result, activity) = Tracker::measure(|| owner.try_freeze());
            let exact = result.expect("the compacted owner is fully initialized");
            assert_eq!(exact.bytes(), 7);
            assert_eq!(activity, [0; 4]);
            let ((), freed) = Tracker::measure(move || {
                assert_eq!(exact.as_slice(), b"payload");
                assert_eq!(exact.as_slice().as_ptr(), pointer);
            });
            assert_eq!((freed[0], freed[2], freed[3]), (0, 1, resized[1]));
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn preserves_partial_owner() {
    Capacity(8)
        .try_scope(|budget| {
            let mut owner =
                (storage::Owned::try_allocate(&budget, 8)).expect("the owner must fit the budget");
            let mut spare = owner.spare();
            assert_eq!(spare.write(b"abc"), 3);
            spare.commit();
            let pointer = owner.as_ptr();
            let (result, activity) = Tracker::measure(|| owner.try_freeze());

            let mut owner = result.expect_err("partial initialization returns its owner");
            assert_eq!(activity, [0; 4]);
            assert_eq!(owner.as_ptr(), pointer);
            assert_eq!(&owner[..], b"abc");
            assert_eq!((owner.capacity(), budget.resident()), (8, 8));
            let mut spare = owner.spare();
            assert_eq!(spare.write(b"defgh"), 5);
            spare.commit();
            let (result, activity) = Tracker::measure(|| owner.try_freeze());
            {
                let exact = result.expect("the completed owner is fully initialized");
                assert_eq!(activity, [0; 4]);
                assert_eq!(exact.as_slice(), b"abcdefgh");
                assert_eq!(exact.as_slice().as_ptr(), pointer);
                assert_eq!(budget.resident(), 8);
            }
            assert_eq!(budget.resident(), 0);
        })
        .expect("resident test domain must admit its budget");
}

#[test]
fn fixed_drop() {
    for retain in [false, true] {
        let drops = Cell::new(Some(0));
        let mut arena = provided::Arena::<_, _, ()>::try_allocate(NonZero::<u32>::MIN)
            .expect("fixed fixture arena")(Fixed {
            len: 1,
            resident: NonZero::<u32>::MIN,
        });
        arena.scope(pin::Pin::new(&mut ()), |_, root, _| {
            let vacant = root.try_vacant().expect("initial fixed vacancy");
            let item = Item::try_new(b"x", &drops).expect("fixture buffer");
            // SAFETY: the item matches the fixed context's one-byte extent and ceiling.
            mem::forget(unsafe { buffer::raw::Fixed::insert(vacant, item) });
            assert_eq!(drops.get(), Some(0));
        });
        assert_eq!(drops.get(), Some(1));
        if retain {
            let context = arena.into_context();
            assert_eq!(context.len, 1);
            assert_eq!(context.resident, NonZero::<u32>::MIN);
        } else {
            assert_eq!(arena.capacity(), NonZero::<u32>::MIN);
            drop(arena);
        }
        assert_eq!(drops.get(), Some(1));
    }
}

#[test]
fn exact_drop() {
    let drops = Cell::new(Some(0));
    {
        let arena =
            provided::Arena::<Item>::try_with(NonZero::<u32>::MIN, || Ok::<(), alloc::Error>(()));
        let mut arena = arena.expect("the exact fixture must allocate");
        arena.scope(pin::Pin::new(&mut ()), |_, root, _| {
            let vacant = root
                .vacant()
                .expect("the exact fixture must admit its first owner");
            let owner = vacant.try_insert(
                Item::try_new(b"x", &drops).expect("fixture buffer must allocate"),
                1,
                Item::bytes_mut,
            );
            let owner = owner.expect("buffer transition accepted");
            mem::forget(owner);
            assert_eq!(drops.get(), Some(0));
        });
        assert_eq!(drops.get(), Some(1));
    }
    assert_eq!(drops.get(), Some(1));
}

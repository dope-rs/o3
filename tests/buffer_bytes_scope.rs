pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{num::NonZero, ops};

use common::{
    alloc::system::{Activity, Tracker},
    buffer::Selected,
};
use o3::{
    alloc,
    buffer::{
        self,
        bytes::{
            scope,
            scope::Source as _,
            window::reserve::{self, Append, Plan, Prefix, Ring},
        },
        pool, resident, storage,
        write::consume,
    },
};

#[test]
fn transfers_frozen() {
    let pool = (pool::Pool::try_new(1, 32)).expect("the head pool must allocate");
    let mut lease = (pool.acquire()).expect("the unused pool must provide its owner");
    let append = (lease.try_append(b"abcdefgh")).expect("the head must fit its owner");
    append.commit();
    scope::Owner::with(lease.freeze(), |owner| {
        let selected = &owner.as_ref()[2..5];
        let pointer = selected.as_ptr();
        let range =
            (owner.view().select(selected)).expect("a head subslice must belong to the head");
        {
            let (body, allocs) = Tracker::count(|| owner.into_shared().project(range));
            assert_eq!(allocs, 0);
            assert_eq!(body.as_slice(), b"cde");
            assert_eq!(body.as_slice().as_ptr(), pointer);
            assert!(pool.acquire().is_none());
        }
        assert!(pool.acquire().is_some());
    });
}

#[test]
fn releases_empty_selection() {
    let pool = (pool::Pool::try_new(1, 32)).expect("the head pool must allocate");
    let mut lease = (pool.acquire()).expect("the unused pool must provide its owner");
    let append = (lease.try_append(b"abcdefgh")).expect("the head must fit its owner");
    append.commit();
    scope::Owner::with(lease.freeze(), |owner| {
        let range = (owner.view().select(&owner.as_ref()[2..2]))
            .expect("an empty head subslice must belong to the head");
        let (body, allocs) = Tracker::count(|| owner.into_shared().project(range));
        assert_eq!(allocs, 0);
        assert!(body.is_empty());
        assert!(pool.acquire().is_some());
    });
}

#[test]
fn rejects_foreign_storage() {
    let first = storage::Shared::try_copy(b"same bytes").expect("the first owner must allocate");
    let second = storage::Shared::try_copy(b"same bytes").expect("the second owner must allocate");
    scope::Owner::with(first, |first| {
        assert!(first.view().select(second.as_slice()).is_none());
        assert!(first.view().select(first.as_ref()).is_some());
    });
}

#[test]
fn selects_nested_bounds() {
    let source = &b"abcdef"[..];
    let view = buffer::View::new(&source);
    let nested = view.select(&source[1..5]).expect("contained selection");
    assert_eq!(nested.as_slice(), b"bcde");
    assert!(nested.select(&source[..1]).is_none());
    assert!(nested.select(&source[4..]).is_none());
    let empty = nested.select(&source[5..5]).expect("empty endpoint");
    assert_eq!(empty.as_slice().as_ptr(), source[5..].as_ptr());
    assert!(empty.is_empty());
    scope::Owner::with(source.to_vec(), |owner| {
        let selected = owner.view().try_get(1..5).expect("contained bounds");
        assert_eq!(owner.project(selected), b"bcde");
    });
}

#[test]
fn retains_vec_backing() {
    let bytes = Vec::from(&b"abcdef"[..]);
    let pointer = bytes.as_ptr();
    let capacity = bytes.capacity();
    let (empty, activity) = Tracker::measure(|| {
        scope::Owner::with(bytes, |owner| {
            let range = owner.view().try_get(3..3).expect("empty interior range");
            owner.project(range)
        })
    });
    assert_eq!(activity, [0; 4]);
    assert!(empty.is_empty());
    assert_eq!(empty.as_ptr(), pointer);
    assert_eq!(empty.capacity(), capacity);
}

#[test]
fn returns_rejected_owners() {
    for bounds in [
        ops::Range { start: 5, end: 4 },
        0..7,
        7..7,
        usize::MAX..usize::MAX,
    ] {
        let shared = storage::Shared::try_copy(b"abcdef").expect("fixture owner");
        let pointer = shared.as_slice().as_ptr();
        let (result, activity) = Tracker::measure(|| shared.project(bounds.clone()));
        assert_eq!(activity, [0; 4]);

        assert!(result.is_err(), "invalid bounds");
        let Err(error) = result else { return };
        let (shared, error) = error.into_parts();
        assert_eq!(
            (error.start(), error.end(), error.len()),
            (bounds.start, bounds.end, 6)
        );
        assert_eq!(shared.as_slice(), b"abcdef");
        assert_eq!(shared.as_slice().as_ptr(), pointer);
        let shared = resident::Shared::from(shared);
        let (result, activity) = Tracker::measure(|| shared.project(bounds.clone()));
        assert_eq!(activity, [0; 4]);

        assert!(result.is_err(), "invalid resident bounds");
        let Err(error) = result else { return };
        let (shared, error) = error.into_parts();
        assert_eq!(
            (error.start(), error.end(), error.len()),
            (bounds.start, bounds.end, 6)
        );
        assert_eq!(shared.as_slice(), b"abcdef");
        assert_eq!(shared.as_slice().as_ptr(), pointer);
    }
}

#[test]
fn honors_policy_capacity() {
    let plan =
        (Plan::<1>::new(NonZero::<usize>::MIN.saturating_add(7))).expect("the ring plan must fit");
    let mut ring = (Ring::try_new(plan)).expect("the ring must allocate");
    Append::try_new(&mut ring, [b"ab".as_slice()], 8)
        .map(Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    let result = Append::try_new(&mut ring, [&b"cde"[..]], 4);
    assert!(
        result.is_err(),
        "the policy must bound admission to four bytes"
    );
    let Err(error) = result else { return };
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(2), 2)
    );
    assert_eq!(ring.as_slices(), (&b"ab"[..], &[][..]));
    let append =
        (Append::try_new(&mut ring, [&b"cd"[..]], 4)).expect("the exact policy suffix must fit");
    append.commit();
    Append::try_new(&mut ring, [b"efgh".as_slice()], 8)
        .map(Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(ring.as_slices(), (&b"abcdefgh"[..], &[][..]));
    let result = Append::try_new(&mut ring, [&b"i"[..]], usize::MAX);
    assert!(
        result.is_err(),
        "policy capacity cannot exceed physical capacity"
    );
    let Err(error) = result else { return };
    assert_eq!(error.into_parts(), (NonZero::<usize>::MIN, 0));
    assert_eq!(ring.as_slices(), (&b"abcdefgh"[..], &[][..]));
}

#[test]
fn retains_wrapped_sources() {
    let plan =
        (Plan::<1>::new(NonZero::<usize>::MIN.saturating_add(6))).expect("the ring plan must fit");
    let mut ring = (Ring::try_new(plan)).expect("the ring must allocate");
    Append::try_new(&mut ring, [b"abcdef".as_slice()], 8)
        .map(Append::commit)
        .as_ref()
        .expect("required fixture outcome");
    let read = Prefix::new(&mut ring, 5);
    assert_eq!(read.len(), 5);
    read.commit();

    Append::try_new(&mut ring, [&b"gh"[..], &b"ijkl"[..]], 8)
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(ring.as_slices(), (&b"f"[..], &[][..]));

    let result = Append::try_new(&mut ring, [&b"gh"[..], &b"ijklm"[..]], 8);
    assert!(
        result.is_err(),
        "all sources must fit before anything is written"
    );
    let Err(error) = result else { return };
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(4), 4)
    );
    assert_eq!(ring.as_slices(), (&b"f"[..], &[][..]));

    let append = (Append::try_new(&mut ring, [&b"gh"[..], &b"ijkl"[..]], 8))
        .expect("the same six bytes must still fit after cancellation");
    append.commit();
    assert_eq!(ring.as_slices(), (&b"fgh"[..], &b"ijkl"[..]));
}

#[test]
fn trailer_keeps_borrow() {
    let source = [1, 2, 3, 4, 5, 6];
    let owner = resident::Shared::from(source.as_slice());
    let result = scope::Owner::with(owner, |owner| {
        let (tail, range) = owner.view().try_rsplit::<4>()?;
        assert_eq!(tail.as_ptr(), source[2..].as_ptr());
        let header = u32::from_le_bytes(*tail);
        Ok::<_, consume::Error>((header, owner.project(range)))
    });
    let (header, payload) = (result).expect("trailer fits");
    assert_eq!(header, 0x0605_0403);
    assert_eq!(payload.as_slice(), &[1, 2]);
    assert_eq!(payload.as_ptr(), source.as_ptr());
}

#[test]
fn vec_keeps_allocation() {
    let source = vec![1, 2, 3, 4, 5, 6];
    let pointer = source.as_ptr();
    let capacity = source.capacity();
    let result = scope::Owner::with(source, |owner| {
        let (tail, range) = owner.view().try_rsplit::<4>()?;
        let header = u32::from_le_bytes(*tail);
        Ok::<_, consume::Error>((header, owner.project(range)))
    });
    let (header, payload) = (result).expect("trailer fits");
    assert_eq!(header, 0x0605_0403);
    assert_eq!(payload, [1, 2]);
    assert_eq!(payload.as_ptr(), pointer);
    assert_eq!(payload.capacity(), capacity);
}

#[test]
fn shared_keeps_owner() {
    let source =
        (storage::Shared::try_copy(&[1, 2, 3, 4, 5, 6])).expect("source allocation succeeds");
    let pointer = source.as_ptr();
    let result = scope::Owner::with(source, |owner| {
        let (tail, range) = owner.view().try_rsplit::<4>()?;
        let header = tail[0];
        Ok::<_, consume::Error>((header, owner.project(range)))
    });
    let (header, payload) = (result).expect("trailer fits");
    assert_eq!(header, 3);
    assert_eq!(payload.as_slice(), &[1, 2]);
    assert_eq!(payload.as_ptr(), pointer);
}

#[test]
fn clones_scoped_ranges() {
    let source = storage::Shared::try_copy(b"abcdef").expect("source allocation");
    let pointer = source.as_ptr();
    let source = resident::Shared::from(source);
    let ((left, right), activity) = Tracker::measure(|| {
        scope::Owner::with(source, |owner| {
            let cloned = owner.try_clone().expect("same source retention");
            let first = Selected::try_new(&owner, 1..3).expect("first selection");
            let second = Selected::try_new(&owner, 3..6).expect("second selection");
            let (bytes, first) = owner.view().get(first);
            assert_eq!(bytes, b"bc");
            let second = owner.view().get(second).1;
            (cloned.project(first), owner.project(second))
        })
    });
    assert_eq!(activity, [0; 4]);
    let bytes = scope::Owner::with(&left, |owner| owner.borrow().as_slice());
    assert_eq!(bytes, b"bc");
    assert_eq!(bytes.as_ptr(), pointer.wrapping_add(1));
    assert_eq!(right.as_slice(), b"def");
    assert_eq!(right.as_ptr(), pointer.wrapping_add(3));
}

#[test]
fn finishes_in_place() {
    let plan = (reserve::Plan::<17>::new(NonZero::<usize>::MIN.saturating_add(31)))
        .expect("the ring plan must fit");
    let mut ring = (reserve::Ring::try_new(plan))
        .expect("the ring must allocate before measuring publication");
    let (result, activity) = Tracker::activity(|| {
        reserve::Append::try_new(&mut ring, [&b"head"[..], &b"body"[..]], 32)?.commit();
        assert_eq!(ring.as_slices(), (&b"headbody"[..], &[][..]));
        ring.finish(&[42; 17]);
        ring.finish(&[99; 17]);
        Ok::<_, consume::Error>(())
    });
    result.as_ref().expect("required fixture outcome");
    assert!(activity == Activity::default());
    assert_eq!(ring.len(), 25);
    assert_eq!(&ring.as_slices().0[8..], &[42; 17]);
}

#[test]
fn allocates_one_buffer() {
    let plan = (reserve::Plan::<17>::new(NonZero::<usize>::MIN.saturating_add(31)))
        .expect("the ring plan must fit");
    let (failed, activity) =
        Tracker::activity(|| Tracker::fail_after(0, || reserve::Ring::try_new(plan)));
    assert!(matches!(failed, Err(alloc::Exhausted)));
    assert!(activity == Activity::default());
    let (created, activity) = Tracker::activity(|| reserve::Ring::try_new(plan));
    let ring = (created).expect("the ring must allocate after failure injection ends");
    assert_eq!(activity.allocations, 1);
    assert_eq!(activity.allocated_bytes, 49);
    assert_eq!(activity.deallocations, 0);
    let ((), activity) = Tracker::activity(move || assert!(ring.is_empty()));
    assert_eq!(activity.allocations, 0);
    assert_eq!(activity.deallocations, 1);
    assert_eq!(activity.deallocated_bytes, 49);
}

#[test]
fn window_keeps_regions() {
    use o3::base::num::bounded;

    let at = NonZero::new(2).expect("nonzero offset");
    let mut short = [0, 1, 2, 3, 4, 5];
    assert!(scope::raw::Window::<2, 3>::try_at(&mut short, at).is_none());
    assert_eq!(short, [0, 1, 2, 3, 4, 5]);

    let mut bytes = [0, 1, 2, 3, 4, 5, 6, 7];
    let mut window = scope::raw::Window::<2, 3>::try_at(&mut bytes, at).expect("complete sample");
    let (first, number, sample) = window.parts();
    *first = 9;
    *number = [10, 11];
    assert_eq!(sample, &[4, 5, 6]);
    let (head, body) = window.split(bounded::U64::<0, 1>::mask(0));
    assert_eq!(head, &[9, 1, 10]);
    assert_eq!(body, &[11, 4, 5, 6, 7]);
    head[1] = 12;
    body[0] = 13;
    assert_eq!(bytes, [9, 12, 10, 13, 4, 5, 6, 7]);
}

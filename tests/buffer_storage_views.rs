pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::{cell::Cell, hint, mem, num::NonZero, pin::Pin};

use common::{
    alloc::system::{Activity, Tracker},
    buffer::provided::{Fixed, Input, Item},
};
use o3::{
    alloc,
    buffer::{
        self,
        storage::{self, Str, provided},
    },
    mem::Rejected,
};

#[test]
fn preserves_returns() {
    let drops = Cell::new(Some(0));
    let mut arena =
        provided::Arena::<_, _, ()>::try_allocate(NonZero::<u32>::MIN.saturating_add(1))
            .expect("the two-slot fixed arena must allocate")(Fixed {
            len: 2,
            resident: NonZero::<u32>::MIN.saturating_add(3),
        });
    let first = Item::try_new(b"abcd", &drops).expect("fixture buffer must allocate");
    let pointer = first.bytes.as_ptr();
    let second = Item::try_new(b"wxyz", &drops).expect("fixture buffer must allocate");
    let ((), stats) = Tracker::activity(|| {
        arena.scope(Pin::new(&mut ()), |_, arena, _| {
            let left = arena
                .try_vacant()
                .expect("the first slot must be available");
            let right = arena
                .try_vacant()
                .expect("the second slot must be available");
            {
                let (returned, stats) = Tracker::activity(|| {
                    {
                        let other;
                        {
                            // SAFETY: both four-byte items admit this unchanged context's
                            // two-byte prefix within its four-byte resident ceiling.
                            let first = unsafe { buffer::raw::Fixed::insert(left, first) };
                            // SAFETY: the second item has the same admitted fixed layout.
                            other = unsafe { buffer::raw::Fixed::insert(right, second) };
                            assert_eq!(first.as_slice(), b"ab");
                            assert_eq!(other.as_slice(), b"wx");
                        }
                    }
                    arena.pop_returned()
                });
                assert_eq!(stats.allocations, 0);
                assert_eq!(stats.deallocations, 0);
                assert!(matches!(&returned, Some(item)
                    if &*item.bytes == b"abcd" && item.bytes.as_ptr() == pointer));
            }
            assert_eq!(drops.get(), Some(1));
        })
    });
    assert_eq!(drops.get(), Some(2));
    assert_eq!(stats.allocations, 0);
    assert_eq!(stats.deallocations, 2);
    let (context, stats) = Tracker::activity(|| arena.into_context());
    assert_eq!(context.len, 2);
    assert_eq!(stats.allocations, 0);
    assert_eq!(stats.deallocations, 1);
}

#[test]
fn recovers_exhaustion() {
    let drops = Cell::new(Some(0));
    let mut arena = provided::Arena::<_, _, ()>::try_allocate(NonZero::<u32>::MIN)
        .expect("one-slot arena must allocate")(Fixed {
        len: 2,
        resident: NonZero::<u32>::MIN.saturating_add(3),
    });
    arena.scope(Pin::new(&mut ()), |_, arena, _| {
        {
            let vacant = arena.try_vacant();
            assert!(vacant.is_ok(), "initial vacancy must exist");
            assert!(
                matches!(arena.try_vacant(), Err(full) if full.capacity == NonZero::<u32>::MIN)
            );
        }
        {
            let vacant = arena
                .try_vacant()
                .expect("dropping a vacancy must return its slot");
            let item = Item::try_new(b"abcd", &drops).expect("fixture buffer must allocate");
            // SAFETY: this fixed context projects two initialized bytes from the
            // four-byte item and retains its unchanged four-byte resident ceiling.
            let first = unsafe { buffer::raw::Fixed::insert(vacant, item) };
            assert_eq!(first.as_slice(), b"ab");
            assert!(
                matches!(arena.try_vacant(), Err(full) if full.capacity == NonZero::<u32>::MIN)
            );
        }
        assert!(arena.has_returned());
        assert!(matches!(arena.try_vacant(), Err(full) if full.capacity == NonZero::<u32>::MIN));
        let first = arena.pop_returned().expect("the dropped item returns");
        assert_eq!(&*first.bytes, b"abcd");
        let vacant = arena
            .try_vacant()
            .expect("taking the returned item must recycle its exact slot");
        // SAFETY: this returned item retains its original arena's fixed layout.
        let (second, stats) =
            Tracker::activity(|| unsafe { buffer::raw::Fixed::insert(vacant, first) });
        assert_eq!(stats.allocations, 0);
        assert_eq!(stats.deallocations, 0);
        assert_eq!(second.as_slice(), b"ab");
    });
    assert_eq!(drops.get(), Some(1));
}

#[test]
fn retains_admission() {
    let drops = Cell::new(Some(0));
    let count = Cell::new(Some(0));
    let mut arena =
        provided::Arena::<Item>::try_with(NonZero::<u32>::MIN, || Ok::<(), alloc::Error>(()))
            .expect("the single-slot arena must allocate");
    let input = Input {
        buffer: Item::try_new(b"body", &drops).expect("fixture buffer must allocate"),
        converted: &count,
    };
    let pointer = input.buffer.bytes.as_ptr();
    arena.scope(Pin::new(&mut ()), |_, root, _| {
        let slot = root
            .vacant()
            .expect("the first admission owns the vacant slot");
        let (result, activity) = Tracker::activity(|| slot.try_adopt(input, 2));
        assert_eq!((activity.allocations, activity.deallocations), (0, 0));
        let rejected = result.expect_err("the two-byte ceiling rejects four bytes");
        let (input, cause) = rejected.into_parts();
        assert_eq!((cause.projected(), cause.resident()), (4, 2));
        assert_eq!(input.buffer.bytes.as_ptr(), pointer);
        assert!(std::ptr::eq(input.converted, &count));
        assert_eq!((count.get(), drops.get()), (Some(0), Some(0)));
        let slot = root.vacant().expect("rejection returns its original slot");
        let (result, activity) = Tracker::activity(|| slot.try_adopt(input, 4));
        assert_eq!((activity.allocations, activity.deallocations), (0, 0));
        {
            let mut owner = result
                .map_err(Rejected::discard)
                .expect("buffer transition accepted");
            assert_eq!((count.get(), drops.get()), (Some(1), Some(0)));
            assert_eq!(owner.as_slice().as_ptr(), pointer);
            owner.as_mut_slice()[0] = b'B';
        }
        {
            let returned = root.pop_returned();
            assert!(matches!(&returned, Some(item)
                if item.bytes.as_ptr() == pointer && &*item.bytes == b"Body"));
        }
        assert_eq!((count.get(), drops.get()), (Some(1), Some(1)));
        assert!(
            root.vacant().is_some(),
            "the completed admission must recycle its slot"
        );
    });
}

#[test]
fn text_keeps_owner() {
    let owner =
        storage::Shared::try_from(&mut String::from("vectorized utf8 boundary").into_bytes())
            .expect("UTF-8 owner backing must allocate");
    let pointer = owner.as_ptr();
    let (text, valid) = Tracker::count(|| storage::Str::from_utf8(owner));
    let text = (text).expect("ASCII test text is valid UTF-8");
    assert_eq!(valid, 0);

    let ((), access) = Tracker::count(|| {
        assert_eq!(hint::black_box(text.as_str()), "vectorized utf8 boundary");
        assert_eq!(hint::black_box(text.as_bytes()).as_ptr(), pointer);
    });
    assert_eq!(access, 0);
    let bad = storage::Shared::try_from(&mut vec![0xff]).expect("invalid UTF-8 owner");
    assert!(Str::from_utf8(bad).is_err());
}

#[test]
fn exact_text_keeps_owner() {
    let owner = storage::Exact::try_copy(b"vectorized utf8 boundary")
        .expect("exact UTF-8 owner backing must allocate");
    let pointer = owner.as_slice().as_ptr();
    let (text, allocs) = Tracker::count(|| Str::from_utf8(owner));
    let text = text.expect("ASCII text is valid UTF-8");
    assert_eq!(allocs, 0);
    assert_eq!(text.as_str(), "vectorized utf8 boundary");
    assert_eq!(text.as_bytes().as_ptr(), pointer);
    assert_eq!(mem::size_of_val(&text), mem::size_of::<usize>());
    let invalid = storage::Exact::try_copy(&[0xff]).expect("exact invalid text allocation");
    assert!(Str::from_utf8(invalid).is_err());
}

#[test]
fn split_keeps_owned_tail() {
    let source = (storage::Shared::try_copy(b"abcdef")).expect("source allocation");
    let start = source.as_ptr();
    let (split, activity) = Tracker::activity(|| source.try_split::<0>());
    assert!(activity == Activity::default());
    let (head, source) = split.expect("an empty prefix preserves the complete owner");
    assert!(head.is_empty());
    assert_eq!(source.as_slice(), b"abcdef");
    assert_eq!(source.as_ptr(), start);
    let (split, activity) = Tracker::activity(|| source.try_split::<2>());
    assert!(activity == Activity::default());
    let (head, tail) = (split).expect("the fixed prefix fits");
    assert_eq!(head, *b"ab");
    assert_eq!(tail.as_slice(), b"cdef");
    assert_eq!(tail.as_ptr(), start.wrapping_add(2));
    let (head, tail) = (tail.try_split::<4>()).expect("the exact remaining prefix fits");
    assert_eq!(head, *b"cdef");
    assert!(tail.is_empty());
    assert!(tail.backing().is_none());
    let (split, activity) = Tracker::activity(|| tail.try_split::<0>());
    assert!(activity == Activity::default());
    let (head, tail) = split.expect("an exhausted owner admits an empty prefix");
    assert!(head.is_empty());
    assert!(tail.is_empty());
    assert!(tail.backing().is_none());
    let result = tail.try_split::<1>();
    assert!(result.is_err(), "the exhausted owner has no prefix");
    let Err(cause) = result else { return };
    assert_eq!(cause.into_parts(), (NonZero::<usize>::MIN, 0));
}

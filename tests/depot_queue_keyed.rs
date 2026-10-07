pub mod common;

use std::{mem, num::NonZero};

use common::depot::{
    index::Id,
    queue::keyed::{Error, Selected, Slots},
};
use o3::depot::{
    self,
    capacity::Count,
    queue::indexed::keyed::{self, Fifo, Link},
    raw::{Key as _, KeySource as _},
};

#[test]
fn keeps_fifo_layout() {
    assert_eq!(mem::size_of::<Link<Id>>(), 8);
    assert_eq!(mem::align_of::<Link<Id>>(), 4);
    assert_eq!(mem::size_of::<Fifo<Id>>(), 8);
    assert_eq!(mem::align_of::<Fifo<Id>>(), 4);
    assert_eq!(mem::size_of::<keyed::lanes::Fifo<Id, 2>>(), 16);
    assert_eq!(mem::align_of::<keyed::lanes::Fifo<Id, 2>>(), 4);
}

#[test]
fn requeues_and_unlinks() {
    let mut slots = Slots::try_occupied(Count::new(4)).expect("four fixture slots");

    slots
        .try_seed::<false>(0..3)
        .expect("seed cursor must be vacant");
    assert!(matches!(
        slots.try_seed::<true>([1]),
        Err(Error::Linked(key))
            if key == Id::new(1, NonZero::<u32>::MIN)
    ));
    assert!(slots.queues::<true>().fifo().is_empty());

    let cursor =
        (keyed::Slot::front(slots.queues::<false>())).expect("the three-member queue has a front");
    assert!((cursor.key()) == (Id::new(0, NonZero::<u32>::MIN)));
    let cursor = cursor.move_to_back();
    cursor.move_to_back();
    let mut cursor =
        (keyed::Slot::front(slots.queues::<false>())).expect("requeue retains its members");
    for index in [1, 2] {
        assert!((cursor.key()) == (Id::new(index, NonZero::<u32>::MIN)));
        let result = cursor.advance();
        let live = result
            .map_err(|cursor| cursor.key().index())
            .expect("cursor must have a successor");
        let live = unsafe {
            live.map(|(selected, index)| {
                (
                    keyed::Slot::from_source(depot::raw::Proof::new(selected))
                        .remove()
                        .release()
                        .into_inner(),
                    index,
                )
            })
        };
        cursor = keyed::Slot::from_source(unsafe {
            depot::raw::Proof::new(depot::raw::KeyPlane::select(live))
        });
    }
    assert!((cursor.key()) == (Id::new(0, NonZero::<u32>::MIN)));
    let tail = cursor
        .advance()
        .err()
        .expect("a tail returns its original removal capability");
    tail.remove().release();
    assert!(slots.queues::<false>().fifo().is_empty());
}

#[test]
fn preserves_failed_state() {
    let mut slots = Slots::try_occupied(Count::new(3)).expect("three fixture slots");
    slots
        .try_seed::<false>(0..3)
        .expect("seed cursor must be vacant");
    let cursor =
        (keyed::Slot::front(slots.queues::<false>())).expect("the initialized queue has a front");
    let cursor = keyed::Slot::try_vacant(cursor.release())
        .err()
        .expect("a member rejects insertion and returns its membership");
    assert!((cursor.key()) == (Id::new(0, NonZero::<u32>::MIN)));
    let vacant = keyed::Slot::try_new(cursor.remove().release())
        .err()
        .expect("a detached slot rejects removal and returns its vacancy");
    let cursor = vacant.push::<false>();
    assert!((cursor.key()) == (Id::new(0, NonZero::<u32>::MIN)));
    cursor.remove().push::<true>();
    for index in 0..3 {
        let cursor = (keyed::Slot::front(slots.queues::<false>()))
            .expect("the next fixture member must remain linked");
        assert!((cursor.key()) == (Id::new(index, NonZero::<u32>::MIN)));
        cursor.remove().release();
    }
    assert!(slots.queues::<false>().fifo().is_empty());
}

#[test]
fn transfers_between_roots() {
    for size in 1..=4 {
        let mut slots = Slots::try_occupied(Count::new(size as u32)).expect("fixture slots");
        slots
            .try_seed::<false>(0..size)
            .expect("seed cursor must be vacant");
        let mut cursor =
            (keyed::Slot::front(slots.queues::<false>())).expect("every fixture has a member");
        for _ in 0..4096 {
            cursor = cursor.transfer();
            assert!((cursor.key()) == (Id::new(0, NonZero::<u32>::MIN)));
            cursor = cursor.transfer();
        }
        cursor.remove().push::<true>();
        assert!(slots.queues::<true>().fifo().is_empty());
        for index in 0..size {
            let cursor = (keyed::Slot::front(slots.queues::<false>()))
                .expect("the next fixture member must remain linked");
            assert!((cursor.key()) == (Id::new(index as u32, NonZero::<u32>::MIN)));
            cursor.remove().release();
        }
        assert!(slots.queues::<false>().fifo().is_empty());
    }
}

#[test]
fn preserves_proof_layout() {
    assert_eq!(
        mem::size_of::<keyed::Slot<Selected<'_>>>(),
        mem::size_of::<Selected<'_>>()
    );
    assert_eq!(
        mem::size_of::<keyed::Slot<Selected<'_>, (), false>>(),
        mem::size_of::<Selected<'_>>()
    );
    assert_eq!(
        mem::align_of::<keyed::Slot<Selected<'_>>>(),
        mem::align_of::<Selected<'_>>()
    );
    assert_eq!(
        mem::align_of::<keyed::Slot<Selected<'_>, (), false>>(),
        mem::align_of::<Selected<'_>>()
    );
}

#[test]
fn traverses_projection() {
    for size in 0..=4 {
        let mut slots = Slots::try_occupied(Count::new(size as u32)).expect("fixture slots");
        slots
            .try_seed::<false>((0..size).rev())
            .expect("seed cursor must be vacant");
        let source = slots.queues::<false>();
        let mut next = if size == 0 {
            keyed::Fifo::first(source)
        } else {
            // SAFETY: seeding linked at least one initialized slot in this retained source.
            Some(keyed::Fifo::front(unsafe {
                depot::raw::Proof::new(source)
            }))
        };
        for index in (0..size).rev() {
            let live = (next).expect("owned traversal must retain every member");
            // SAFETY: Queues shares only stable slot metadata; the projection grants no mutation.
            assert!(
                (unsafe { live.as_ref() }.into().key)
                    == (Id::new(index as u32, NonZero::<u32>::MIN))
            );
            next = keyed::Iter::successor(live);
        }
        assert!(next.is_none());
        let mut entries = keyed::Iter::<_, (), false>::new(slots.queues::<false>());
        for index in (0..size).rev() {
            let live = (entries.next_entry()).expect("every linked slot must be yielded");
            let node = live.into();
            assert!((node.key) == (Id::new(index as u32, NonZero::<u32>::MIN)));
        }
        assert!(entries.next_entry().is_none());
        assert!(entries.next_entry().is_none());
        let mut entries = keyed::Iter::<_, (), true>::new(slots.queues::<false>());
        for index in 0..size {
            let live = (entries.next_entry()).expect("reverse traversal must retain every member");
            let node = live.into();
            assert!((node.key) == (Id::new(index as u32, NonZero::<u32>::MIN)));
        }
        assert!(entries.next_entry().is_none());
        assert!(entries.next_entry().is_none());
    }
}

#[test]
fn appends_whole_chains() {
    for left in 0..=4 {
        for right in 0..=4 {
            let mut slots =
                Slots::try_occupied(Count::new((left + right) as u32)).expect("fixture slots");
            slots
                .try_seed::<true>(0..left)
                .expect("seed cursor must be vacant");
            slots
                .try_seed::<false>(left..left + right)
                .expect("seed cursor must be vacant");
            keyed::Fifo::append(slots.queues::<false>());
            assert!(slots.queues::<false>().fifo().is_empty());
            for index in 0..left + right {
                let cursor = (keyed::Slot::front(slots.queues::<true>()))
                    .expect("both chains retain all members");
                assert!((cursor.key()) == (Id::new(index as u32, NonZero::<u32>::MIN)));
                cursor.remove().release();
            }
            assert!(slots.queues::<true>().fifo().is_empty());
        }
    }
}

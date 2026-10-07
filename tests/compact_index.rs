pub mod common;

use std::ptr;

use common::depot::compact::{Read, Remove, Store};
use o3::{
    alloc::tail,
    depot::map::compact::{self, Access as _, Insert as _},
};

#[test]
fn alloc_empty_index() {
    for (capacity, start) in [(0, 0), (0, 7), (1, 7), (257, 7), (65_537, 7)] {
        let (layout, end) = compact::Layout::try_new(capacity, start).expect("layout fits");
        let plan = compact::raw::Plan::new(layout).expect("allocation fits");
        let block = plan.try_allocate(91_u64).expect("allocation succeeds");
        let view = tail::View::new(&block);
        assert_eq!(*view.header(), 91);
        assert_eq!(view.as_slice().len(), end);
        assert_eq!(layout.byte(end), 0);
        assert!(
            view.as_slice()
                .iter()
                .enumerate()
                .all(|(offset, byte)| *byte == layout.byte(offset))
        );
    }
}

#[test]
fn rejects_invalid_layouts() {
    let maximum = 7 << 28;
    compact::Layout::try_new(maximum, 0)
        .as_ref()
        .expect("required fixture outcome");
    let error = compact::Layout::try_new(maximum + 1, 0)
        .err()
        .expect("capacity must fit");
    assert_eq!(
        error,
        compact::Error::Capacity {
            actual: maximum + 1,
            maximum
        }
    );
    let start = u32::MAX as usize + 1;
    let error = compact::Layout::try_new(1, start)
        .err()
        .expect("the offset must fit");
    assert_eq!(
        error,
        compact::Error::Start {
            actual: start,
            maximum: u32::MAX as usize
        }
    );
}

#[test]
fn admits_payload_widths() {
    for (capacity, bytes) in [
        (1, 96),
        (256, 1056),
        (257, 1568),
        (65_536, 393_248),
        (65_537, 655_392),
    ] {
        let (_, end) = compact::Layout::try_new(capacity, 0).expect("the layout must fit");
        assert_eq!(end, bytes);
        let mut store = Store::try_new(capacity).expect("fixture layout must fit");
        let payload = capacity - 1;
        let hash = payload as u64;
        let ready = store
            .try_insert(payload, hash)
            .expect("the record must fit");
        assert_eq!(ready.insert(), payload);
        assert_eq!(store.try_find(hash).expect("scan allocation"), [payload]);
        assert_eq!(store.try_remove(hash, payload), Ok(Some(hash)));
        assert_eq!(store.try_remove(hash, payload), Ok(None));
    }
}

#[test]
fn shifts_wrapped_clusters() {
    let mut store = Store::try_new(28).expect("fixture layout must fit");
    for payload in 0..28 {
        let hash = if payload % 3 == 0 { 31 } else { 63 };
        store
            .try_insert(payload, hash)
            .expect("each record is unique")
            .insert();
    }
    for payload in (0..28).step_by(2) {
        let hash = if payload % 3 == 0 { 31 } else { 63 };
        assert_eq!(store.try_remove(hash, payload), Ok(Some(hash)));
    }
    for hash in [31, 63] {
        let expected: Vec<_> = (0..28)
            .filter(|p| p % 2 == 1 && (if p % 3 == 0 { 31 } else { 63 }) == hash)
            .collect();
        assert_eq!(store.try_find(hash).expect("scan allocation"), expected);
    }
    for payload in (0..28).step_by(2) {
        store
            .try_insert(payload, 95)
            .expect("removed records are reusable")
            .insert();
    }
    assert_eq!(
        store.try_find(95).expect("scan allocation"),
        (0..28).step_by(2).collect::<Vec<_>>()
    );
}

#[test]
fn searches_empty_domain() {
    let mut store = Store::try_new(0).expect("fixture layout must fit");
    assert!(store.try_find(0).expect("empty scan").is_empty());
    assert_eq!(store.try_remove(0, 0), Ok(None));
}

#[test]
fn retains_displaced_slots() {
    let mut store = Store::try_new(48).expect("fixture layout must fit");
    let mut expected = [None; 48];
    for turn in 0..768usize {
        let payload = turn.wrapping_mul(29) % expected.len();
        if let Some(hash) = expected[payload].take() {
            assert_eq!(store.try_remove(hash, payload), Ok(Some(hash)));
        }
        let hash = (turn as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let ready = store
            .try_insert(payload, hash)
            .expect("the slot was released");
        assert_eq!(ready.insert(), payload);
        expected[payload] = Some(hash);
        for (payload, hash) in expected.iter().enumerate() {
            if let Some(hash) = hash {
                assert_eq!(store.try_find(*hash).expect("scan allocation"), [payload]);
            }
        }
    }
}

#[test]
fn entry_retains_owner() {
    for capacity in [0, 28, 224, 1792] {
        let store = Store::try_new(capacity).expect("fixture layout must fit");
        let original = ptr::from_ref(&store);
        let entry = Read(&store).entry(31, &0);
        let owner = Read::vacant(entry).expect("a missing query must retain its owner");
        assert!(ptr::eq(owner.0, original));
    }
}

#[test]
fn entry_selects_collision() {
    for capacity in [28, 257, 65_537] {
        let mut store = Store::try_new(capacity).expect("fixture layout must fit");
        for payload in [0, 1, capacity - 1] {
            store
                .try_insert(payload, 31)
                .expect("unique record")
                .insert();
        }
        let original = ptr::from_ref(&store);
        let entry = Read(&store).entry(31, &(capacity - 1));
        let occupied = match entry {
            compact::Entry::Occupied(occupied) => Some(occupied),
            compact::Entry::Vacant(_) => None,
        }
        .expect("the key must select the exact colliding record");
        assert_eq!(occupied.value().index(), capacity - 1);
        assert!(ptr::eq(occupied.into_owner().0, original));
        let entry = Read(&store).entry(31, &2);
        let owner = Read::vacant(entry)
            .expect("exhausting colliding candidates must return the original owner");
        assert!(ptr::eq(owner.0, original));
        let occupied = Remove(&mut store)
            .find(31, &1)
            .expect("the mutable query must retain its removal capability");
        assert_eq!(occupied.remove(), Ok(31));
        assert_eq!(
            store.try_find(31).expect("scan allocation"),
            [0, capacity - 1]
        );
    }
}

#[test]
fn searches_wrapped_groups() {
    for (capacity, first) in
        (1..32)
            .map(|first| (56, first))
            .chain([(56, 40), (257, 40), (65_537, 40)])
    {
        let home = u64::from(u32::MAX);
        let query = (2 << 57) | home;
        for other in [query ^ (1 << 32), (1 << 57) | home] {
            let mut store = Store::try_new(capacity).expect("layout fits");
            for offset in 0..48 {
                let hash = if offset < first { other } else { query };
                store
                    .try_insert(capacity - 1 - offset, hash)
                    .expect("distinct slot")
                    .insert();
            }
            let expected: Vec<_> = (first..48).map(|offset| capacity - 1 - offset).collect();
            let mut visited = Vec::new();
            let owner = Read(&store).scan(query, |value| visited.push(value));
            assert!(ptr::eq(owner.0, &store));
            assert_eq!(visited, expected);
            assert_eq!(
                store.hash_reads.get(),
                if other >> 57 == query >> 57 {
                    48
                } else {
                    expected.len()
                }
            );
            for payload in expected {
                let occupied = match Read(&store).entry(query, &payload) {
                    compact::Entry::Occupied(occupied) => Some(occupied),
                    compact::Entry::Vacant(_) => None,
                }
                .expect("a later group retains the matching record");
                assert_eq!(occupied.value().index(), payload);
                assert!(ptr::eq(occupied.into_owner().0, &store));
            }
            let owner = Read::vacant(Read(&store).entry(query, &(capacity - 1)))
                .expect("a tag collision must not match a different full hash");
            assert!(ptr::eq(owner.0, &store));
        }
    }
}

const _: () = {
    assert!(size_of::<compact::Found<Remove<'_>>>() == 2 * size_of::<usize>());
    assert!(size_of::<Option<compact::Found<Remove<'_>>>>() == 2 * size_of::<usize>());
    assert!(size_of::<compact::Entry<Read<'_>>>() == 2 * size_of::<usize>());
};

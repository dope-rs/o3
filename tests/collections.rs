use o3::collections::Slab;
use o3::collections::{CellQueue, FixedHashTable, FixedQueue, IndexedMinHeap, SlotQueue};
use std::cell::Cell;
use std::cmp::Ordering;

use crate::support::PanicDrop;

struct PanicOrd<'a> {
    order: u8,
    panic_once: &'a Cell<bool>,
    drops: &'a Cell<usize>,
}

impl Drop for PanicOrd<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

impl PartialEq for PanicOrd<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.order == other.order
    }
}

impl Eq for PanicOrd<'_> {}

impl PartialOrd for PanicOrd<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PanicOrd<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.panic_once.replace(false) {
            panic!("comparison panic");
        }
        self.order.cmp(&other.order)
    }
}

fn assert_panicking_drop_finishes<'a>(
    drops: &'a Cell<usize>,
    panic_once: &'a Cell<bool>,
    drop_collection: impl FnOnce(PanicDrop<'a>, PanicDrop<'a>),
) {
    drops.set(0);
    panic_once.set(true);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        drop_collection(
            PanicDrop::new(0, drops, panic_once),
            PanicDrop::new(1, drops, panic_once),
        );
    }));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 2);
}

fn push_pair<T>(first: T, second: T, mut push: impl FnMut(T)) {
    push(first);
    push(second);
}

enum DropQueue<T> {
    Fixed(FixedQueue<T>),
    Cell(CellQueue<T>),
}

impl<T> DropQueue<T> {
    fn with_capacity(cell: bool, capacity: usize) -> Self {
        if cell {
            Self::Cell(CellQueue::with_capacity(capacity))
        } else {
            Self::Fixed(FixedQueue::with_capacity(capacity))
        }
    }

    fn push_back(&mut self, value: T) {
        let inserted = match self {
            Self::Fixed(queue) => queue.push_back(value).is_ok(),
            Self::Cell(queue) => queue.push_back(value).is_ok(),
        };
        assert!(inserted);
    }
}

#[test]
fn slot_queue_preserves_index_order_and_membership() {
    let mut queue = SlotQueue::with_capacity(2);
    queue.vacant_entry(1).unwrap().push_back("one");
    assert_eq!(queue.push_back(1, "again"), Err("again"));
    assert_eq!(queue.push_back(2, "outside"), Err("outside"));
    assert!(queue.push_front(0, "zero").is_ok());
    assert_eq!(queue.front_key_value(), Some((0, &"zero")));
    assert_eq!(queue.remove(1), Some("one"));
    assert!(!queue.contains_key(1));
    assert_eq!(queue.pop_front_key_value(), Some((0, "zero")));
    assert!(queue.is_empty());

    assert!(queue.push_back(1, "one").is_ok());
    assert!(queue.push_front(0, "zero").is_ok());
    queue.grow_to(4);
    assert_eq!(queue.capacity(), 4);
    assert_eq!(queue.pop_front_key_value(), Some((0, "zero")));
    assert_eq!(queue.pop_front_key_value(), Some((1, "one")));
}

#[test]
fn indexed_collections_reject_reused_slab_keys() {
    let mut slab: Slab<()> = Slab::with_capacity(1);
    let first = slab.insert(()).unwrap();

    let mut heap = IndexedMinHeap::with_capacity(1);
    heap.insert(first, 7).unwrap();
    assert_eq!(slab.remove(first), Some(()));
    let second = slab.insert(()).unwrap();
    assert_ne!(first, second);

    assert!(!heap.contains_key(second));
    assert_eq!(heap.remove(second), None);
    assert_eq!(heap.remove(first), Some(7));
}

#[test]
fn bounded_queues_are_fifo() {
    let mut queue = FixedQueue::with_capacity(3);
    assert!(queue.push_back(1).is_ok());
    assert!(queue.push_back(2).is_ok());
    assert!(queue.contains(&1));
    assert_eq!(queue.pop_front(), Some(1));
    assert!(!queue.contains(&1));
    assert!(queue.push_back(3).is_ok());
    assert!(queue.push_back(4).is_ok());
    assert_eq!(queue.pop_front(), Some(2));
    assert_eq!(queue.pop_front(), Some(3));
    assert_eq!(queue.pop_front(), Some(4));
    assert_eq!(queue.pop_front(), None);

    let queue = CellQueue::with_capacity(3);
    assert_eq!(queue.capacity(), 3);
    assert!(queue.push_back(1).is_ok());
    assert!(queue.push_back(2).is_ok());
    assert_eq!(queue.pop_front(), Some(1));
    assert!(queue.push_back(3).is_ok());
    assert!(queue.push_back(4).is_ok());
    assert!(queue.push_back(5).is_err());
    assert_eq!(queue.pop_front(), Some(2));
    assert_eq!(queue.pop_front(), Some(3));
    assert_eq!(queue.pop_front(), Some(4));
    assert_eq!(queue.pop_front(), None);
}

#[test]
fn fixed_hash_table_reuses_wrapped_clusters() {
    let mut table: FixedHashTable<(u32, u32)> = FixedHashTable::with_capacity(8);
    for epoch in 0..256u32 {
        for key in 0..8u32 {
            assert_eq!(
                table.try_insert(15, (epoch, key), |entry| entry.1 == key),
                Ok(())
            );
        }
        for key in [3, 0, 7, 1, 6, 2, 5, 4] {
            assert_eq!(table.remove(15, |entry| entry.1 == key), Some((epoch, key)));
        }
        assert!(table.is_empty());
    }
}

#[test]
fn fixed_hash_table_owns_non_copy_values() {
    let mut table = FixedHashTable::with_capacity(2);
    assert_eq!(table.insert(7, String::from("first"), |_| false), Ok(None));
    assert_eq!(
        table.try_insert(7, String::from("duplicate"), |value| value == "first"),
        Err(String::from("duplicate"))
    );
    assert_eq!(
        table.get(7, |value| value == "first").map(String::as_str),
        Some("first")
    );
    table
        .get_mut(7, |value| value == "first")
        .unwrap()
        .push_str(" value");
    for value in table.values_mut() {
        value.push('!');
    }
    assert_eq!(
        table.insert(7, String::from("second"), |value| value == "first value!"),
        Ok(Some(String::from("first value!")))
    );
    let cloned = table.clone();
    assert_eq!(
        cloned.get(7, |value| value == "second").map(String::as_str),
        Some("second")
    );
    assert_eq!(format!("{cloned:?}"), "[\"second\"]");
    assert_eq!(
        table.remove(7, |value| value == "second"),
        Some(String::from("second"))
    );
}

#[test]
fn fixed_hash_table_clear_restores_state_after_drop_panics() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let mut table = FixedHashTable::with_capacity(2);
    assert!(
        table
            .try_insert(0, PanicDrop::new(0, &drops, &panic_once), |_| false,)
            .is_ok()
    );
    assert!(
        table
            .try_insert(1, PanicDrop::new(1, &drops, &panic_once), |_| false,)
            .is_ok()
    );
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| table.clear()));
    assert!(caught.is_err());
    assert_eq!(table.len(), 1);
    assert!(table.remove(0, |_| true).is_some() || table.remove(1, |_| true).is_some());
    assert!(table.is_empty());
}

#[test]
fn fixed_queue_wrap_math_handles_zst_capacity() {
    let mut queue = FixedQueue::with_capacity(usize::MAX);
    queue.push_front(()).unwrap();
    queue.push_back(()).unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.pop_front(), Some(()));
    assert_eq!(queue.pop_front(), Some(()));
}

#[test]
fn indexed_heap_matches_its_std_model_under_churn() {
    let mut state = 1u64;
    let mut indexed = IndexedMinHeap::with_capacity(64);
    let mut indexed_model = [None; 64];

    indexed.vacant_entry(0).unwrap().insert((9, 0));
    indexed.insert(1, (4, 1)).unwrap();
    assert_eq!(indexed.peek(), Some((1, &(4, 1))));
    assert_eq!(indexed.remove(0), Some((9, 0)));
    assert_eq!(indexed.remove(1), Some((4, 1)));

    let iterations = if cfg!(miri) { 500 } else { 10_000 };
    for _ in 0..iterations {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        let index = (state as usize >> 16) & 63;
        let key = ((state >> 32) as u32, index);
        match state & 3 {
            0 | 1 => {
                let result = indexed.insert(index, key);
                if indexed_model[index].is_some() {
                    assert_eq!(result, Err(key));
                } else {
                    indexed_model[index] = Some(key);
                    assert_eq!(result, Ok(()));
                }
            }
            2 => {
                assert_eq!(indexed.remove(index), indexed_model[index].take());
            }
            _ => {
                let expected = indexed_model
                    .iter()
                    .enumerate()
                    .filter_map(|(index, key)| key.map(|key| (index, key)))
                    .min_by_key(|(_, key)| *key);
                assert_eq!(indexed.peek().map(|(index, key)| (index, *key)), expected);
            }
        }
    }
}

#[test]
fn heap_holes_close_when_comparison_panics() {
    let panic_once = Cell::new(false);
    let drops = Cell::new(0);
    let mut heap = IndexedMinHeap::with_capacity(3);
    heap.insert(
        0,
        PanicOrd {
            order: 0,
            panic_once: &panic_once,
            drops: &drops,
        },
    )
    .ok();
    panic_once.set(true);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        heap.insert(
            1,
            PanicOrd {
                order: 1,
                panic_once: &panic_once,
                drops: &drops,
            },
        )
        .ok();
    }));
    assert!(caught.is_err());
    drop(heap);
    assert_eq!(drops.get(), 2);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn fixed_collections_keep_their_thin_layouts() {
    assert_eq!(std::mem::size_of::<FixedQueue<u64>>(), 32);
    assert_eq!(std::mem::size_of::<SlotQueue<u64>>(), 32);
    assert_eq!(std::mem::size_of::<Slab<u64>>(), 40);
}

#[test]
fn indexed_min_heap_clear_keeps_positions_coherent_across_unwind() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let mut heap = IndexedMinHeap::with_capacity(2);
    heap.insert(0, PanicDrop::new(0, &drops, &panic_once)).ok();
    heap.insert(1, PanicDrop::new(1, &drops, &panic_once)).ok();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| heap.clear()));
    assert!(caught.is_err());
    assert_eq!(heap.len(), 1);
    assert!(heap.contains_key(0));
    assert!(!heap.contains_key(1));
    assert!(heap.remove(0).is_some());
    assert!(heap.is_empty());
}

#[test]
fn slab_clear_survives_a_drop_panic() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(false);
    let mut slab: Slab<PanicDrop<'_>> = Slab::with_capacity(2);
    slab.insert(PanicDrop::new(0, &drops, &panic_once)).ok();
    slab.insert(PanicDrop::new(1, &drops, &panic_once)).ok();
    panic_once.set(true);
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| slab.clear()));
    assert!(caught.is_err());
    assert!(slab.len() <= 1);
    let key = slab
        .insert(PanicDrop::new(2, &drops, &panic_once))
        .map_err(drop)
        .expect("a slot is free");
    assert!(slab.get(key).is_some());
}

#[test]
fn collection_drop_finishes_after_one_element_panics() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);

    assert_panicking_drop_finishes(&drops, &panic_once, |first, second| {
        let mut slab: Slab<PanicDrop<'_>> = Slab::with_capacity(2);
        slab.insert(first).ok();
        slab.insert(second).ok();
        drop(slab);
    });
    for cell in [false, true] {
        assert_panicking_drop_finishes(&drops, &panic_once, |first, second| {
            let mut queue = DropQueue::with_capacity(cell, 2);
            push_pair(first, second, |value| queue.push_back(value));
            drop(queue);
        });
    }
    assert_panicking_drop_finishes(&drops, &panic_once, |first, second| {
        let mut queue = SlotQueue::with_capacity(2);
        queue.push_back(0, first).ok();
        queue.push_back(1, second).ok();
        drop(queue);
    });
}

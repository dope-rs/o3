use std::{cell::Cell, cmp::Ordering, collections::VecDeque};

use o3::{
    collections::{
        fixed::{
            arena::{Linked, Stack},
            array::CopyInline,
            hash::{Map, Plan},
            index::Slots,
        },
        heap::Min,
        queue::{fixed, round::Robin, slot},
        slab::{Capacity, Exclusive},
    },
    queue,
};

use crate::support::PanicDrop;

mod sealed;

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

#[test]
fn copy_array_vec_has_no_drop_glue() {
    let mut values = CopyInline::<u64, 4>::new();
    assert_eq!(values.push(1), Ok(()));
    assert_eq!(values.push(2), Ok(()));
    assert!(!std::mem::needs_drop::<CopyInline<u64, 4>>());
    assert_eq!(values.as_slice(), [1, 2]);
}

enum DropQueue<T> {
    Fixed(fixed::Fifo<T>),
    Cell(queue::Fifo<T>),
}

impl<T> DropQueue<T> {
    fn with_capacity(cell: bool, capacity: usize) -> Self {
        if cell {
            Self::Cell(queue::Fifo::with_capacity(capacity))
        } else {
            Self::Fixed(fixed::Fifo::with_capacity(capacity))
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
    let mut queue = slot::Fifo::with_capacity(4);
    queue.vacant_entry(1).unwrap().push_back("one");
    assert_eq!(queue.push_back(1, "again"), Err("again"));
    assert_eq!(queue.push_back(4, "outside"), Err("outside"));
    queue.vacant_entry(0).unwrap().push_front("zero");
    assert_eq!(queue.front_key_value(), Some((0, &"zero")));
    let front = queue.front_entry().unwrap();
    assert_eq!(front.index(), 0);
    assert_eq!(front.remove(), "zero");
    assert_eq!(queue.remove(1), Some("one"));
    assert!(queue.is_empty());

    assert!(queue.push_back(1, "one").is_ok());
    assert!(queue.push_front(0, "zero").is_ok());
    assert_eq!(queue.capacity(), 4);
    assert_eq!(queue.pop_front_key_value(), Some((0, "zero")));
    assert_eq!(queue.pop_front_key_value(), Some((1, "one")));

    assert!(queue.push_back(0, "first").is_ok());
    assert!(queue.push_back(1, "second").is_ok());
    assert_eq!(queue.remove_if(0, |value| *value == "stale"), None);
    assert_eq!(queue.front_key_value(), Some((0, &"first")));
    assert_eq!(queue.remove_if(0, |value| *value == "first"), Some("first"));
    assert_eq!(queue.pop_front_key_value(), Some((1, "second")));
}

#[test]
fn slot_queue_refreshes_an_entry_at_the_back() {
    let mut queue = slot::Fifo::with_capacity(2);
    assert_eq!(queue.push_back(0, "old"), Ok(()));
    assert_eq!(queue.push_back(1, "one"), Ok(()));

    assert_eq!(queue.refresh_back(1, "tail"), Ok(Some("one")));
    assert_eq!(queue.front_key_value(), Some((0, &"old")));
    assert_eq!(queue.refresh_back(0, "new"), Ok(Some("old")));
    assert_eq!(queue.front_key_value(), Some((1, &"tail")));
    assert_eq!(queue.pop_front_key_value(), Some((1, "tail")));
    assert_eq!(queue.pop_front_key_value(), Some((0, "new")));
    assert_eq!(queue.refresh_back(0, "vacant"), Ok(None));
    assert_eq!(queue.pop_front_key_value(), Some((0, "vacant")));
    assert_eq!(queue.refresh_back(2, "outside"), Err("outside"));
}

#[test]
fn cell_slot_queue_preserves_shared_index_order_and_membership() {
    let queue = slot::Cell::with_capacity(4);
    assert_eq!(queue.push_back(1, 11), Ok(()));
    assert_eq!(queue.push_back(1, 12), Err(12));
    assert_eq!(queue.push_back(4, 20), Err(20));
    assert_eq!(queue.remove_if(1, |value| *value == 12), None);
    assert_eq!(queue.remove_if(1, |value| *value == 11), Some(11));
    assert!(queue.is_empty());

    assert_eq!(queue.push_back(1, 11), Ok(()));
    assert_eq!(queue.remove(1), Some(11));
    assert_eq!(queue.push_back(1, 11), Ok(()));
    assert_eq!(queue.pop_front(), Some(11));
    assert_eq!(queue.push_back(3, 13), Ok(()));
    assert_eq!(queue.pop_front(), Some(13));
}

#[test]
fn indexed_heap_growth_preserves_live_order_and_positions() {
    let mut heap = Min::with_capacity(2);
    assert_eq!(heap.insert(0, 30), Ok(()));
    assert_eq!(heap.insert(1, 10), Ok(()));

    heap.grow_to(4);

    assert_eq!(heap.capacity(), 4);
    assert_eq!(heap.insert(2, 20), Ok(()));
    assert_eq!(heap.pop(), Some((1, 10)));
    assert_eq!(heap.remove(0), Some(30));
    assert_eq!(heap.pop(), Some((2, 20)));
}

#[test]
fn bounded_queues_are_fifo() {
    let mut queue = fixed::Fifo::with_capacity(3);
    assert!(queue.is_empty());
    assert!(queue.push_back(1).is_ok());
    assert!(queue.push_back(2).is_ok());
    assert_eq!(queue.len(), 2);
    assert!(queue.iter().any(|value| value == &1));
    assert_eq!(queue.pop_front(), Some(1));
    assert!(!queue.iter().any(|value| value == &1));
    assert!(queue.push_back(3).is_ok());
    assert!(queue.push_back(4).is_ok());
    assert!(queue.is_full());
    assert_eq!(queue.len(), 3);
    assert_eq!(queue.push_back(5), Err(5));
    assert_eq!(queue.pop_front(), Some(2));
    assert_eq!(queue.pop_front(), Some(3));
    assert_eq!(queue.pop_front(), Some(4));
    assert_eq!(queue.pop_front(), None);

    let queue = queue::Fifo::with_capacity(3);
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
fn bounded_queue_pop_reserves_its_vacancy() {
    let mut queue = fixed::Fifo::with_capacity(2);
    assert_eq!(queue.push_back(1), Ok(()));
    assert_eq!(queue.push_back(2), Ok(()));

    let (value, vacant) = queue
        .pop_front_reserved()
        .expect("occupied queue must yield a value and its vacancy");
    assert_eq!(value, 1);
    vacant.push_front(value);

    assert_eq!(queue.pop_front(), Some(1));
    assert_eq!(queue.pop_front(), Some(2));
    assert!(queue.pop_front_reserved().is_none());
}

#[test]
fn coalescing_queue_keeps_one_position_per_index() {
    let mut queue = fixed::Coalescing::with_capacity(2);
    assert_eq!(queue.schedule(1, "old"), Ok(()));
    assert_eq!(queue.schedule(0, "other"), Ok(()));
    assert_eq!(queue.schedule(1, "new"), Ok(()));
    assert_eq!(queue.schedule(2, "outside"), Err("outside"));
    assert_eq!(queue.len(), 2);

    assert_eq!(queue.pop_front(), Some((1, "new")));
    assert_eq!(queue.pop_front(), Some((0, "other")));
    assert_eq!(queue.pop_front(), None);
}

#[test]
fn round_robin_set_rotates_and_unlinks() {
    let mut set = Robin::with_capacity(4);
    assert!(set.insert(1));
    assert!(set.insert(3));
    assert!(!set.insert(1));
    assert_eq!(set.next_index(), Some(1));
    assert_eq!(set.next_index(), Some(3));
    assert_eq!(set.next_index(), Some(1));
    assert!(set.remove(1));
    assert_eq!(set.next_index(), Some(3));
    assert!(set.remove(3));
    assert_eq!(set.next_index(), None);
}

#[test]
fn fixed_hash_table_reuses_wrapped_clusters() {
    assert!(Plan::<u8>::new(8).is_some());
    assert!(Plan::<u8>::new(usize::MAX).is_none());
    let plan = Plan::new(8).unwrap();
    assert_eq!(plan.capacity(), 8);
    let mut table: Map<(u32, u32)> = Map::from_plan(plan);
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
    let plan = Plan::new(2).unwrap();
    let mut table = Map::from_plan(plan);
    assert_eq!(
        table.try_insert(7, String::from("first"), |_| false),
        Ok(())
    );
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
        table.values().map(String::as_str).collect::<Vec<_>>(),
        ["first value!"]
    );
    let cloned = table.clone();
    assert_eq!(
        cloned
            .get(7, |value| value == "first value!")
            .map(String::as_str),
        Some("first value!")
    );
    assert_eq!(format!("{cloned:?}"), "[\"first value!\"]");
    assert_eq!(
        table.remove(7, |value| value == "first value!"),
        Some(String::from("first value!"))
    );
}

#[test]
fn fixed_hash_table_clear_restores_state_after_drop_panics() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let plan = Plan::new(2).unwrap();
    let mut table = Map::from_plan(plan);
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
fn fixed_index_table_addresses_sparse_values_without_probing() {
    let mut table = Slots::with_capacity(130);
    assert_eq!(table.try_insert(0, String::from("zero")), Ok(()));
    assert_eq!(table.try_insert(64, String::from("sixty-four")), Ok(()));
    assert_eq!(table.try_insert(129, String::from("last")), Ok(()));
    assert_eq!(
        table.try_insert(130, String::from("outside")),
        Err(String::from("outside"))
    );
    assert_eq!(
        table.try_insert(64, String::from("occupied")),
        Err(String::from("occupied"))
    );

    assert_eq!(table.get(64).map(String::as_str), Some("sixty-four"));

    let mut drained = Vec::new();
    table.drain_where(|value| value != "zero", |value| drained.push(value));
    assert_eq!(drained, ["sixty-four", "last"]);
    assert_eq!(table.remove(0), Some(String::from("zero")));
    assert!(table.is_empty());
}

#[test]
fn fixed_index_table_clear_restores_state_after_drop_panics() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let mut table = Slots::with_capacity(65);
    table
        .try_insert(0, PanicDrop::new(0, &drops, &panic_once))
        .ok();
    table
        .try_insert(64, PanicDrop::new(1, &drops, &panic_once))
        .ok();

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(table)));
    assert!(caught.is_err());
    assert_eq!(drops.get(), 2);
}

#[test]
fn fixed_queue_wrap_math_handles_zst_capacity() {
    let mut queue = fixed::Fifo::with_capacity(usize::MAX);
    queue.push_front(()).unwrap();
    queue.push_back(()).unwrap();
    assert_eq!(queue.len(), 2);
    assert_eq!(queue.pop_front(), Some(()));
    assert_eq!(queue.pop_front(), Some(()));
}

#[test]
fn fixed_queue_matches_vec_deque_under_mixed_wraparound() {
    let mut fixed = fixed::Fifo::with_capacity(17);
    let mut model = VecDeque::with_capacity(17);
    let mut state = 1u64;
    for _ in 0..10_000 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
        match state >> 61 {
            0 | 1 => {
                let value = state as u32;
                let actual = fixed.push_back(value);
                let expected = if model.len() == 17 {
                    Err(value)
                } else {
                    model.push_back(value);
                    Ok(())
                };
                assert_eq!(actual, expected);
            }
            2 | 3 => {
                let value = state as u32;
                let actual = fixed.push_front(value);
                let expected = if model.len() == 17 {
                    Err(value)
                } else {
                    model.push_front(value);
                    Ok(())
                };
                assert_eq!(actual, expected);
            }
            4 | 5 => assert_eq!(fixed.pop_front(), model.pop_front()),
            _ => {
                let parity = state as u32 & 1;
                fixed.retain(|value| value & 1 == parity);
                model.retain(|value| value & 1 == parity);
            }
        }
        assert_eq!(fixed.len(), model.len());
        assert_eq!(
            fixed.iter().copied().collect::<Vec<_>>(),
            model.iter().copied().collect::<Vec<_>>()
        );
        assert_eq!(fixed.front(), model.front());
        assert_eq!(fixed.is_empty(), model.is_empty());
        assert_eq!(fixed.is_full(), model.len() == 17);
    }
}

#[test]
fn indexed_heap_matches_its_std_model_under_churn() {
    let mut state = 1u64;
    let mut indexed = Min::with_capacity(64);
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
fn indexed_heap_sets_and_conditionally_pops_in_one_operation() {
    use o3::collections::slab::{Capacity, Exclusive};

    let mut indexed = Min::with_capacity(1);
    let mut slots: Exclusive<()> = Exclusive::with_capacity(Capacity::new(3));
    let first = slots.slots().vacant_entry_at(0).unwrap().insert(());
    let last = slots.slots().vacant_entry_at(2).unwrap().insert(());

    indexed.set(first, 9);
    indexed.set(last, 7);
    assert_eq!(indexed.capacity(), 3);
    assert_eq!(indexed.peek(), Some((2, &7)));
    indexed.set(last, 11);
    assert_eq!(indexed.peek(), Some((0, &9)));
    indexed.set(first, 4);
    assert_eq!(indexed.pop_if(|key| *key > 4), None);
    assert_eq!(indexed.pop_if(|key| *key == 4), Some((0, 4)));
    assert_eq!(indexed.pop(), Some((2, 11)));
}

#[test]
fn heap_holes_close_when_comparison_panics() {
    let panic_once = Cell::new(false);
    let drops = Cell::new(0);
    let mut heap = Min::with_capacity(3);
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

#[test]
fn fixed_collections_keep_their_thin_layouts() {
    if usize::BITS == 64 {
        assert_eq!(std::mem::size_of::<queue::Fifo<u64>>(), 40);
        assert_eq!(std::mem::size_of::<fixed::Fifo<u64>>(), 32);
        assert_eq!(std::mem::size_of::<fixed::Coalescing<u32>>(), 48);
        assert_eq!(std::mem::size_of::<Map<u64>>(), 64);
        assert_eq!(std::mem::size_of::<Slots<u64>>(), 40);
        assert_eq!(std::mem::size_of::<slot::Fifo<u64>>(), 32);
        assert_eq!(std::mem::size_of::<slot::Cell<u64>>(), 32);
        assert_eq!(std::mem::size_of::<Exclusive<u64>>(), 40);
        assert_eq!(std::mem::size_of::<Robin>(), 32);
    }
}

#[test]
fn indexed_min_heap_clear_keeps_positions_coherent_across_unwind() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(true);
    let mut heap = Min::with_capacity(2);
    heap.insert(0, PanicDrop::new(0, &drops, &panic_once)).ok();
    heap.insert(1, PanicDrop::new(1, &drops, &panic_once)).ok();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| heap.clear()));
    assert!(caught.is_err());
    assert_eq!(heap.len(), 1);
    assert!(heap.remove(0).is_some());
    assert!(heap.is_empty());
}

#[test]
fn slab_clear_survives_a_drop_panic() {
    let drops = Cell::new(0);
    let panic_once = Cell::new(false);
    let mut slab: Exclusive<PanicDrop<'_>> = Exclusive::with_capacity(Capacity::new(2));
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
        let mut slab: Exclusive<PanicDrop<'_>> = Exclusive::with_capacity(Capacity::new(2));
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
        let mut queue = slot::Fifo::with_capacity(2);
        queue.push_back(0, first).ok();
        queue.push_back(1, second).ok();
        drop(queue);
    });
    assert_panicking_drop_finishes(&drops, &panic_once, |first, second| {
        let mut arena = Linked::with_capacity(2, 1);
        arena.push_back(0, first).ok();
        arena.push_back(0, second).ok();
        drop(arena);
    });
    assert_panicking_drop_finishes(&drops, &panic_once, |first, second| {
        let mut arena = Stack::with_capacity(2, 1);
        arena.push(0, first).ok();
        arena.push(0, second).ok();
        drop(arena);
    });
    assert_panicking_drop_finishes(&drops, &panic_once, |first, second| {
        let mut table = Slots::with_capacity(2);
        table.try_insert(0, first).ok();
        table.try_insert(1, second).ok();
        drop(table);
    });
}

#[test]
fn linked_arena_mutates_a_lane_front_without_relinking_it() {
    let mut arena = Linked::with_capacity(3, 2);
    arena.push_back(0, String::from("head")).unwrap();
    arena.push_back(0, String::from("tail")).unwrap();
    arena.push_back(1, String::from("other")).unwrap();

    let front = arena.front(0).unwrap() as *const String;
    arena.front_mut(0).unwrap().push_str("-mutated");

    assert_eq!(arena.front(0).unwrap(), "head-mutated");
    assert_eq!(arena.front(0).unwrap() as *const String, front);
    assert_eq!(arena.lane_len(0), 2);
    assert_eq!(arena.lane_len(1), 1);
}

#[test]
fn linked_front_entry_changes_the_lane_only_when_taken() {
    let mut arena = Linked::with_capacity(2, 1);
    arena.push_back(0, 1).unwrap();
    arena.push_back(0, 2).unwrap();

    drop(arena.front_entry(0).expect("front"));
    assert_eq!(arena.lane_len(0), 2);

    let front = arena.front_entry(0).expect("front");
    assert_eq!(front.take(), 1);
    assert_eq!(arena.pop_front(0), Some(2));
}

#[test]
fn linked_arena_represents_an_inert_zero_lane_configuration_without_allocation() {
    let mut arena = Linked::<u8>::with_capacity(0, 0);

    assert!(arena.is_full());
    assert!(arena.push_back(0, 1).is_err());
    assert!(arena.pop_front(0).is_none());
    assert!(arena.front(0).is_none());
    assert!(arena.front_mut(0).is_none());
}

#[test]
fn stack_arena_shares_capacity_and_dropped_drains_reclaim_the_lane() {
    let mut arena = Stack::with_capacity(3, 2);
    arena.push(0, String::from("first")).unwrap();
    arena.push(0, String::from("second")).unwrap();
    arena.push(1, String::from("other")).unwrap();
    let mut drain = arena.drain(0);
    assert_eq!(drain.next().as_deref(), Some("second"));
    drop(drain);
    assert!(arena.lane_is_empty(0));
    assert_eq!(arena.available(), 2);
    assert_eq!(arena.pop(1).as_deref(), Some("other"));
    assert_eq!(arena.available(), 3);
}

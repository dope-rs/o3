use std::{mem, pin};

use o3::{
    collections::batch::{self, Next, Set},
    mem::quota::Ledger,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct Key(u16);

unsafe impl batch::DenseIndex for Key {
    fn into_usize(self) -> usize {
        usize::from(self.0)
    }

    unsafe fn from_usize_unchecked(raw: usize) -> Self {
        Self(raw as u16)
    }
}

#[test]
fn typed_set_preserves_index_identity_and_layout() {
    let set = Set::<Key>::with_capacity(4);
    assert!(set.insert(Key(2)));
    assert_eq!(set.pop(), Some(Key(2)));
    assert_eq!(mem::size_of_val(&set), mem::size_of::<Set>());
    assert_eq!(mem::align_of_val(&set), mem::align_of::<Set>());
}

#[test]
fn located_front_changes_the_set_only_when_taken() {
    let mut set = Set::<Key>::with_capacity(4);
    assert!(set.insert(Key(2)));

    drop(set.front().expect("front"));
    assert!(set.contains(Key(2)));

    let front = set.front().expect("front");
    assert_eq!(front.get(), Key(2));
    assert_eq!(front.take(), Key(2));
    assert!(set.is_empty());
}

#[test]
fn erased_pinned_storage_round_trips_the_typed_index() {
    let set = Box::pin(Set::<Key>::with_capacity(4));
    let raw = unsafe { Set::erase(set.as_ref()) };
    assert!(raw.insert(Key(3).0.into()));
    assert_eq!(set.pop(), Some(Key(3)));

    fn same_lifetime(set: pin::Pin<&Set<Key>>) -> pin::Pin<&batch::RawSet> {
        unsafe { Set::erase(set) }
    }
    let _ = same_lifetime(set.as_ref());
}

#[test]
fn membership_covers_pending_and_draining_batches() {
    let set = Set::with_capacity(3);
    assert!(!set.contains(1));
    assert!(!set.contains(3));

    assert!(set.insert(1));
    assert!(set.contains(1));
    let mut batch = set.drain_batch().unwrap();
    assert!(set.contains(1));
    assert_eq!(batch.next(), Some(1));
    assert!(!set.contains(1));

    assert!(set.insert(1));
    assert!(set.contains(1));
    drop(batch);
    assert_eq!(set.drain_batch().unwrap().collect::<Vec<_>>(), [1]);
    assert!(!set.contains(1));
}

#[test]
fn pop_from_uses_the_requested_start_and_wraps_once() {
    let set = Set::with_capacity(130);
    for index in [3, 65, 129] {
        assert!(set.insert(index));
    }

    assert_eq!(set.pop_from(64), Some(65));
    assert_eq!(set.pop_from(100), Some(129));
    assert_eq!(set.pop_from(100), Some(3));
    assert_eq!(set.pop_from(0), None);
}

#[test]
fn raw_restore_returns_a_removed_index_without_validation() {
    use batch::raw::Set as _;

    let set = Set::with_capacity(4);
    assert!(set.insert(2));
    let index = set.pop().expect("index");
    unsafe { set.restore_unchecked(index) };
    assert_eq!(set.pop(), Some(2));
}

#[test]
fn raw_insert_and_remove_use_proven_membership() {
    use batch::raw::Set as _;

    let set = Set::with_capacity(4);
    assert!(set.insert(1));
    let mut batch = set.drain_batch().expect("batch");
    unsafe { set.remove_unchecked(1) };
    assert_eq!(batch.next(), None);

    unsafe { set.insert_unchecked(2) };
    assert!(set.contains(2));
    unsafe { set.remove_unchecked(2) };
    assert!(!set.contains(2));
}

#[test]
fn coalesces_across_the_draining_and_pending_batches() {
    let set = Set::with_capacity(4);
    assert!(set.insert(0));
    assert!(set.insert(1));

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.next(), Some(0));
    assert!(set.insert(0));
    assert!(!set.insert(1));
    assert_eq!(batch.next(), Some(1));
    assert_eq!(batch.next(), None);
    drop(batch);

    assert_eq!(set.drain_batch().unwrap().collect::<Vec<_>>(), [0]);
    assert!(set.is_empty());
}

#[test]
fn dropping_a_partial_batch_returns_each_index_once() {
    let set = Set::with_capacity(8);
    for index in 0..4 {
        assert!(set.insert(index));
    }

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.next(), Some(0));
    assert!(set.insert(0));
    assert!(!set.insert(2));
    drop(batch);

    let mut returned = set.drain_batch().unwrap().collect::<Vec<_>>();
    returned.sort_unstable();
    assert_eq!(returned, [0, 1, 2, 3]);
    assert!(set.is_empty());
}

#[test]
fn pausing_a_partial_batch_resumes_without_admitting_the_next_batch() {
    let set = Set::with_capacity(130);
    for index in 0..130 {
        assert!(set.insert(index));
    }

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.peek(), Some(0));
    assert_eq!(batch.peek(), Some(0));
    for expected in 0..32 {
        assert_eq!(batch.next(), Some(expected));
    }
    assert_eq!(batch.peek(), Some(32));
    assert!(set.insert(0));
    batch.pause();

    assert_eq!(
        set.drain_batch().unwrap().collect::<Vec<_>>(),
        (32..130).collect::<Vec<_>>()
    );
    assert_eq!(set.drain_batch().unwrap().collect::<Vec<_>>(), [0]);
    assert!(set.is_empty());
}

#[test]
fn quota_gated_drain_preserves_unadmitted_work() {
    enum Work {}

    let set = Set::with_capacity(3);
    assert!(set.insert(1));
    assert!(set.insert(2));
    let mut ledger = Ledger::<Work>::new(1);
    let mut batch = set.drain_batch().unwrap();

    assert!(matches!(batch.next_with_quota(&ledger), Next::Item(1)));
    assert_eq!(ledger.remaining(), 0);
    assert!(matches!(batch.next_with_quota(&ledger), Next::Exhausted(2)));
    assert_eq!(batch.peek(), Some(2));

    ledger.reset(1);
    assert!(matches!(batch.next_with_quota(&ledger), Next::Item(2)));
    ledger.reset(1);
    assert!(matches!(batch.next_with_quota(&ledger), Next::Empty));
    assert_eq!(ledger.remaining(), 1);
}

#[test]
fn removing_a_paused_batch_opens_the_pending_batch_immediately() {
    let set = Set::with_capacity(4);
    assert!(set.insert(0));
    assert!(set.insert(1));

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.next(), Some(0));
    assert!(set.insert(2));
    batch.pause();

    assert!(set.remove(1));
    assert_eq!(set.drain_batch().unwrap().collect::<Vec<_>>(), [2]);
    assert!(set.is_empty());
}

#[test]
fn pausing_an_empty_batch_opens_the_pending_batch() {
    let set = Set::with_capacity(2);
    assert!(set.insert(0));

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.next(), Some(0));
    assert!(set.insert(1));
    batch.pause();

    assert_eq!(set.drain_batch().unwrap().collect::<Vec<_>>(), [1]);
    assert!(set.is_empty());
}

#[test]
fn emptying_a_live_batch_does_not_admit_a_nested_drain() {
    let set = Set::with_capacity(3);
    assert!(set.insert(0));

    let batch = set.drain_batch().unwrap();
    assert!(set.insert(1));
    assert!(set.remove(0));
    assert!(set.drain_batch().is_none());
    drop(batch);

    assert_eq!(set.drain_batch().unwrap().collect::<Vec<_>>(), [1]);
    assert!(set.is_empty());
}

#[test]
fn unwinding_a_partial_batch_restores_each_index_once() {
    let set = Set::with_capacity(4);
    for index in 0..3 {
        assert!(set.insert(index));
    }

    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut batch = set.drain_batch().unwrap();
        assert_eq!(batch.next(), Some(0));
        assert!(set.insert(0));
        panic!("stop partial drain");
    }));
    assert!(unwind.is_err());

    let mut restored = set.drain_batch().unwrap().collect::<Vec<_>>();
    restored.sort_unstable();
    assert_eq!(restored, [0, 1, 2]);
    assert!(set.is_empty());
}

#[test]
fn rejects_nested_drains_without_disturbing_the_live_batch() {
    let set = Set::with_capacity(2);
    assert!(set.insert(1));

    let mut batch = set.drain_batch().unwrap();
    assert!(set.drain_batch().is_none());
    assert_eq!(batch.next(), Some(1));
    assert_eq!(batch.next(), None);
    drop(batch);

    assert!(set.is_empty());
}

#[test]
fn remove_unlinks_an_index_from_either_batch() {
    let set = Set::with_capacity(3);
    assert!(set.insert(0));
    assert!(set.insert(1));

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.next(), Some(0));
    assert!(set.insert(0));
    assert!(set.remove(0));
    assert!(set.remove(1));
    assert!(!set.remove(2));
    assert_eq!(batch.next(), None);
    drop(batch);

    assert!(set.is_empty());
}

#[test]
fn churn_matches_a_reference_set_across_partial_batches() {
    const CAPACITY: usize = 4_097;
    let set = Set::with_capacity(CAPACITY);
    let mut pending = std::collections::BTreeSet::new();
    let mut state = 0x9e37_79b9_7f4a_7c15u64;

    let mut random_index = || {
        state ^= state << 7;
        state ^= state >> 9;
        state ^= state << 8;
        state as usize % CAPACITY
    };

    for _ in 0..128 {
        for _ in 0..64 {
            let index = random_index();
            assert_eq!(set.insert(index), pending.insert(index));
        }

        let mut current = std::mem::take(&mut pending);
        let mut batch = set.drain_batch().unwrap();
        assert!(set.drain_batch().is_none());

        for step in 0..96 {
            let index = random_index();
            match step % 3 {
                0 => {
                    if let Some(index) = batch.next() {
                        assert!(current.remove(&index));
                        if index & 1 == 0 {
                            assert!(set.insert(index));
                            assert!(pending.insert(index));
                        }
                    }
                }
                1 => {
                    let expected = !current.contains(&index) && pending.insert(index);
                    assert_eq!(set.insert(index), expected);
                }
                _ => {
                    let expected = current.remove(&index) || pending.remove(&index);
                    assert_eq!(set.remove(index), expected);
                }
            }
            assert_eq!(set.len(), current.len() + pending.len());
        }

        drop(batch);
        pending.append(&mut current);
        assert_eq!(set.len(), pending.len());
    }

    let mut actual = set.drain_batch().unwrap().collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, pending.into_iter().collect::<Vec<_>>());
    assert!(set.is_empty());
}

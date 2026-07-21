use o3::collections::BatchSet;

#[test]
fn coalesces_across_the_draining_and_pending_batches() {
    let set = BatchSet::with_capacity(4);
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
    let set = BatchSet::with_capacity(8);
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
fn rejects_nested_drains_without_disturbing_the_live_batch() {
    let set = BatchSet::with_capacity(2);
    assert!(set.insert(1));

    let mut batch = set.drain_batch().unwrap();
    assert!(set.drain_batch().is_none());
    assert_eq!(batch.next(), Some(1));
    assert_eq!(batch.next(), None);
    drop(batch);

    assert!(set.is_empty());
}

#[test]
fn grows_both_generations_across_summary_levels() {
    let set = BatchSet::with_capacity(1);
    assert!(set.insert(0));
    set.grow_to(1 << 20);
    assert!(set.insert(900_001));

    let mut batch = set.drain_batch().unwrap();
    assert_eq!(batch.next(), Some(0));
    assert!(set.insert(0));
    assert_eq!(batch.next(), Some(900_001));
    drop(batch);

    assert_eq!(set.pop(), Some(0));
    assert!(set.is_empty());
}

#[test]
fn remove_unlinks_an_index_from_either_batch() {
    let set = BatchSet::with_capacity(3);
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
    let set = BatchSet::with_capacity(CAPACITY);
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

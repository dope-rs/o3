use o3::collections::{Slab, SlabKey};
use std::collections::{BTreeSet, HashMap};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

#[test]
fn free_list_matches_a_reference_set() {
    const CAP: u32 = if cfg!(miri) { 128 } else { 5000 };
    let mut s: Slab<u32> = Slab::with_capacity(CAP as usize);
    let mut free: BTreeSet<u32> = (0..CAP).collect();
    let mut live: HashMap<u32, SlabKey> = HashMap::new();
    let mut rng = Lcg(0x1234_5678_9abc_def0);

    let boundaries: &[u32] = if cfg!(miri) {
        &[0, 63, 64, 127]
    } else {
        &[0, 63, 64, 127, 4095, 4096, 4999]
    };
    for &index in boundaries {
        let first = s.insert_at_with(index, |_| 2).unwrap();
        assert!(free.remove(&index));
        assert_eq!(s.remove(first), Some(2));
        free.insert(index);
        let replacement = s.insert_at_with(index, |_| 2).unwrap();
        assert_ne!(first, replacement);
        assert!(free.remove(&index));
        assert!(live.insert(index, replacement).is_none());
    }

    let iterations = if cfg!(miri) { 1_000 } else { 300_000 };
    for _ in 0..iterations {
        match rng.next() % 3 {
            0 => match s.insert(1) {
                Ok(key) => {
                    let idx = key.index();
                    assert!(free.remove(&idx), "insert returned occupied slot {idx}");
                    assert!(live.insert(idx, key).is_none(), "duplicate insert at {idx}");
                    assert_eq!(s.get(key), Some(&1));
                }
                Err(_) => assert!(free.is_empty(), "insert failed with {} free", free.len()),
            },
            1 => {
                let idx = (rng.next() as u32) % CAP;
                let placed = s.insert_at_with(idx, |_| 2);
                assert_eq!(
                    placed.is_some(),
                    free.contains(&idx),
                    "insert_at_with({idx}) disagreed with model"
                );
                if let Some(key) = placed {
                    free.remove(&idx);
                    live.insert(idx, key);
                }
            }
            _ => {
                let victim = live.keys().next().copied();
                if let Some(idx) = victim {
                    let key = live.remove(&idx).unwrap();
                    assert!(
                        s.remove(key).is_some(),
                        "removing live key {idx} returned None"
                    );
                    free.insert(idx);
                    assert_eq!(s.get(key), None, "removed key {idx} still resolves");
                }
            }
        }
    }

    assert_eq!(free.len() as u32 + live.len() as u32, CAP);
    for (&idx, &key) in &live {
        assert!(s.get(key).is_some(), "live key {idx} vanished");
    }
    let mut drained = 0usize;
    while s.insert(9).is_ok() {
        drained += 1;
    }
    assert_eq!(
        drained,
        free.len(),
        "drained {drained}, model has {} free",
        free.len()
    );
}

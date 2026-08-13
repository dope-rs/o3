use o3::collections::{
    fixed::hash::{self, Entry, Map, Plan},
    queue::fixed,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Index(u32);

unsafe impl fixed::raw::Index for Index {
    fn index(self) -> u32 {
        self.0
    }
}

#[test]
fn coalescing_queue_preserves_typed_indices() {
    let mut queue = fixed::Coalescing::with_index_capacity(2);
    assert_eq!(queue.schedule(Index(1), "value"), Ok(()));
    assert_eq!(queue.pop_front(), Some((Index(1), "value")));
}

#[test]
fn fixed_hash_entries_bind_probe_to_mutation() {
    let plan = Plan::new(1).expect("one entry is a valid plan");
    let mut table = Map::from_plan(plan);

    let entry = table.entry(7, |_| false).expect("vacant entry");
    let Entry::Vacant(entry) = entry else {
        panic!("new key must be vacant")
    };
    entry.insert(String::from("value"));

    assert!(table.entry(9, |_| false).is_none());

    let mut entry = unsafe {
        hash::raw::Map::occupied_entry_unchecked(&mut table, 7, |value| value == "value")
    };
    entry.get_mut().push_str(" updated");
    assert_eq!(entry.get(), "value updated");
    assert_eq!(entry.remove(), "value updated");

    let entry = unsafe { hash::raw::Map::entry_unchecked(&mut table, 11, |_| false) };
    let Entry::Vacant(entry) = entry else {
        panic!("removed slot must be vacant")
    };
    entry.insert(String::from("reused"));
    assert_eq!(table.get(11, |_| true).map(String::as_str), Some("reused"));
}

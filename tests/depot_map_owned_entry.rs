pub mod common;

use common::depot::map::{Aligned, AlignedSeed};
use o3::depot::map::priority::{Entry, Min, Plan, errors::Full};

#[test]
fn retains_owned_key() {
    let plan = Plan::<Aligned, u32, u32>::new(1).expect("plan");
    let mut map = Min::try_new(plan, AlignedSeed).expect("map");
    let vacant = match map.try_entry(Aligned(17)) {
        Ok(Entry::Vacant(entry)) => Some(entry),
        _ => None,
    }
    .expect("vacant");
    vacant.insert(3, 5);
    assert_eq!(map.get(&Aligned(17)), Some(&5));

    let (key, cause) = map.try_entry(Aligned(23)).err().expect("full").into_parts();
    assert_eq!(key.0, 23);
    assert_eq!(cause, Full);
}

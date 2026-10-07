pub mod common;

use std::cell::Cell;

use common::{
    Counted,
    depot::heap::split::{Priority, Split},
};
use o3::depot::heap::{hole, hole::raw::Source as _};

#[test]
fn preserves_split_storage() {
    let drops = Cell::new(Some(0));
    let mut heap = Split::default();
    for (slot, value) in [10, 30, 20, 40, 50, 25, 21].into_iter().enumerate() {
        let priority = Priority {
            value,
            drops: Counted(&drops),
        };
        // SAFETY: each identity is absent and the fixed allocation has spare capacity.
        unsafe { (&mut heap).vacancy((slot, priority)) }.store();
    }
    let writes = heap.writes.expect("write count");
    for value in [22, 22, 5, 21] {
        let position = heap.positions[6].expect("occupied");
        let priority = Priority {
            value,
            drops: Counted(&drops),
        };
        // SAFETY: the reverse map proves this exact owner's position and unchanged identity.
        unsafe {
            hole::raw::Source::update((&mut heap).entry(position, (6, priority)));
        }
        if value == 22 {
            assert_eq!(heap.writes, Some(writes));
        }
    }
    let position = heap.positions[3].expect("occupied");
    unsafe { (&mut heap).entry(position, ()) }.clear();
    assert_eq!(heap.positions[6], Some(1));
    for expected in [10, 20, 21, 25, 30, 50] {
        assert_ne!(heap.len, 0);
        assert_eq!(
            unsafe { heap.priorities[0].assume_init_ref() }.value,
            expected
        );
        unsafe { (&mut heap).entry(0, ()) }.clear();
    }
    assert_eq!(heap.positions, [None; 7]);
    assert_eq!(drops.get(), Some(11));
}

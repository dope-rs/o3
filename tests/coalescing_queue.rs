use o3::collections::queue::coalescing;

#[test]
fn duplicate_indices_coalesce_without_changing_order() {
    let mut queue = coalescing::Fifo::with_capacity(3);
    assert_eq!(queue.schedule(2), Ok(()));
    assert_eq!(queue.schedule(1), Ok(()));
    assert_eq!(queue.schedule(2), Ok(()));
    assert_eq!(queue.len(), 2);

    let front = queue.front_entry().expect("first entry");
    assert_eq!(front.index(), 2);
    front.remove();
    assert_eq!(queue.schedule(2), Ok(()));
    let front = queue.front_entry().expect("second entry");
    assert_eq!(front.index(), 1);
    front.remove();
    let front = queue.front_entry().expect("rescheduled entry");
    assert_eq!(front.index(), 2);
    front.remove();
    assert!(queue.is_empty());
}

#[test]
fn dropped_front_guard_preserves_the_entry() {
    let mut queue = coalescing::Fifo::with_capacity(1);
    assert_eq!(queue.schedule(0), Ok(()));
    {
        let front = queue.front_entry().expect("front entry");
        assert_eq!(front.index(), 0);
    }
    assert_eq!(queue.len(), 1);
    queue.front_entry().expect("retained entry").remove();
}

#[test]
fn capacity_is_enforced_by_the_index_domain() {
    let mut queue = coalescing::Fifo::with_capacity(2);
    assert_eq!(queue.capacity(), 2);
    assert_eq!(queue.schedule(0), Ok(()));
    assert_eq!(queue.schedule(1), Ok(()));
    assert_eq!(queue.schedule(2), Err(2));
    assert_eq!(queue.schedule(0), Ok(()));
    assert_eq!(queue.len(), 2);
}

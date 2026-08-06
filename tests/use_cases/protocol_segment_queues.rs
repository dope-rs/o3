use o3::buffer::{Bytes, Retained, queue::Segments};

#[test]
fn protocol_queues_share_ranges_without_erasing_segment_ownership() {
    let first = Bytes::<Retained>::copy_from_slice(b"frame-");
    let first_allocation = first.as_slice().as_ptr();
    let second = Bytes::<Retained>::copy_from_slice(b"payload");
    let mut queue = Segments::new();
    queue.try_push_back(first).unwrap();
    queue.try_push_back(second).unwrap();

    let (owner, range) = queue
        .contiguous_segment(0, 0, 5)
        .expect("the frame prefix stays in its retained owner");
    let retained = owner.clone().get(range).unwrap();
    assert_eq!(retained.as_slice(), b"frame");
    assert_eq!(retained.as_slice().as_ptr(), first_allocation);

    let mut wire = [0; 13];
    assert!(queue.copy_range_into(0, 0, &mut wire));
    assert_eq!(&wire, b"frame-payload");

    assert!(queue.try_consume_front(6));
    let (owner, range) = queue.contiguous_segment(0, 0, 7).unwrap();
    assert_eq!(&owner.as_slice()[range], b"payload");
    assert_eq!(queue.len(), 7);
}

use std::mem::size_of;

use o3::buffer::{Bytes, Retained, SegmentQueue};

#[cfg(target_pointer_width = "64")]
#[test]
fn queue_keeps_the_deque_and_length_layout() {
    assert_eq!(size_of::<SegmentQueue<Vec<u8>>>(), 40);
}

#[test]
fn ranges_cross_segments_from_a_consumed_front() {
    let mut queue = SegmentQueue::new();
    queue.try_push_back(b"abc".to_vec()).unwrap();
    queue.try_push_back(b"def".to_vec()).unwrap();
    queue.try_push_back(b"ghi".to_vec()).unwrap();

    let mut front_offset = 0;
    let mut removed = Vec::new();
    assert!(queue.try_consume_front_from(&mut front_offset, 2, |segment| removed.push(segment)));
    assert!(removed.is_empty());
    assert_eq!(front_offset, 2);
    assert_eq!(queue.len(), 7);

    let mut copied = [0; 5];
    assert!(queue.copy_range_into(front_offset, 1, &mut copied));
    assert_eq!(&copied, b"defgh");

    assert!(queue.try_consume_front_from(&mut front_offset, 3, |segment| removed.push(segment)));
    assert_eq!(removed, [b"abc".to_vec()]);
    assert_eq!(front_offset, 2);
    assert_eq!(queue.len(), 4);
}

#[test]
fn advancing_consumption_keeps_no_external_cursor() {
    let mut queue = SegmentQueue::new();
    queue
        .try_push_back(Bytes::<Retained>::copy_from_slice(b"first"))
        .unwrap();
    queue
        .try_push_back(Bytes::<Retained>::copy_from_slice(b"second"))
        .unwrap();

    assert!(queue.try_consume_front(3));

    assert_eq!(queue.front().unwrap().as_slice(), b"st");
    assert_eq!(queue.len(), 8);
    let mut bytes = Vec::new();
    assert!(queue.extend_range(0, 0, queue.len(), &mut bytes));
    assert_eq!(bytes, b"stsecond");
}

#[test]
fn contiguous_ranges_retain_their_segment_owner() {
    let first = b"first".to_vec();
    let allocation = first.as_ptr();
    let mut queue = SegmentQueue::new();
    queue.try_push_back(first).unwrap();
    queue.try_push_back(b"second".to_vec()).unwrap();

    let (segment, range) = queue
        .contiguous_segment(0, 1, 3)
        .expect("range stays in the first segment");
    assert_eq!(&segment[range.clone()], b"irs");
    assert_eq!(segment[range].as_ptr(), allocation.wrapping_add(1));
    assert!(queue.contiguous_segment(0, 4, 3).is_none());
}

#[test]
fn mutating_the_tail_updates_aggregate_length() {
    let mut queue = SegmentQueue::new();
    queue.try_push_back(b"head".to_vec()).unwrap();

    queue
        .try_mutate_back(5, |tail| tail.extend_from_slice(b"-tail"))
        .expect("tail exists");

    assert_eq!(queue.len(), 9);
    assert_eq!(queue.back().unwrap(), b"head-tail");
}

#[test]
fn panicking_tail_mutation_restores_aggregate_length() {
    let mut queue = SegmentQueue::new();
    queue.try_push_back(b"head".to_vec()).unwrap();

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        queue.try_mutate_back(5, |tail| {
            tail.extend_from_slice(b"-tail");
            panic!("mutation failed after extending");
        });
    }));

    assert!(panic.is_err());
    assert_eq!(queue.len(), 9);
    assert_eq!(queue.back().unwrap(), b"head-tail");
}

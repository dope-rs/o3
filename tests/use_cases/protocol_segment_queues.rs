use o3::buffer::{Bytes, InlineBytes, Retained, SegmentQueue};

enum TransportSegment {
    Inline(InlineBytes),
    Retained(Bytes<Retained>),
}

impl AsRef<[u8]> for TransportSegment {
    fn as_ref(&self) -> &[u8] {
        match self {
            Self::Inline(bytes) => bytes.as_slice(),
            Self::Retained(bytes) => bytes.as_slice(),
        }
    }
}

#[test]
fn protocol_queues_share_ranges_without_erasing_segment_ownership() {
    let first = Bytes::<Retained>::copy_from_slice(b"frame-");
    let first_allocation = first.as_slice().as_ptr();
    let second = Bytes::<Retained>::copy_from_slice(b"payload");
    let mut queue = SegmentQueue::new();
    queue.try_push_back(first).unwrap();
    queue.try_push_back(second).unwrap();

    let (owner, range) = queue
        .contiguous_segment(0, 0, 5)
        .expect("the frame prefix stays in its retained owner");
    let retained = owner.clone().get(range).unwrap();
    assert_eq!(retained.as_slice(), b"frame");
    assert_eq!(retained.as_slice().as_ptr(), first_allocation);

    let mut wire = Vec::new();
    assert!(queue.extend_range(0, 0, queue.len(), &mut wire));
    assert_eq!(wire, b"frame-payload");

    assert!(queue.try_consume_front(6));
    assert_eq!(queue.front().unwrap().as_slice(), b"payload");
    assert_eq!(queue.len(), 7);
}

#[test]
fn transport_queues_consume_prefixes_without_moving_segment_bytes() {
    let prefix = InlineBytes::from_slice(b"head").unwrap();
    let payload = Bytes::<Retained>::copy_from_slice(b"payload");
    let payload_allocation = payload.as_slice().as_ptr();
    let mut queue = SegmentQueue::new();
    queue
        .try_push_back(TransportSegment::Inline(prefix))
        .unwrap_or_else(|_| unreachable!("small queue length cannot overflow"));
    queue
        .try_push_back(TransportSegment::Retained(payload))
        .unwrap_or_else(|_| unreachable!("small queue length cannot overflow"));

    let mut front_offset = 0;
    assert!(queue.try_consume_front_from(&mut front_offset, 5, drop));

    assert_eq!(front_offset, 1);
    assert_eq!(queue.len(), 6);
    assert_eq!(queue.front().unwrap().as_ref().as_ptr(), payload_allocation);
    let mut tail = Vec::new();
    assert!(queue.extend_range(front_offset, 0, queue.len(), &mut tail));
    assert_eq!(tail, b"ayload");
}

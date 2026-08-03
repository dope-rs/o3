use o3::collections::{CopyArrayVec, FixedIndexTable, StackArena};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Packet {
    number: u64,
    bytes: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ControlHandle(u64);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct StreamHandle(u64);

#[test]
fn quic_packet_metadata_stays_compact_while_typed_carriers_share_fixed_storage() {
    let mut packets = FixedIndexTable::with_capacity(4);
    let mut controls = StackArena::with_capacity(4, 4);
    let mut streams = StackArena::with_capacity(8, 4);
    let mut packet_controls = CopyArrayVec::<ControlHandle, 16>::new();
    let mut packet_streams = CopyArrayVec::<StreamHandle, 16>::new();
    let packet = Packet {
        number: 5,
        bytes: 1_200,
    };
    let lane = packet.number as usize % packets.capacity();

    packet_controls.push(ControlHandle(1)).unwrap();
    packet_streams.push(StreamHandle(2)).unwrap();
    packet_streams.push(StreamHandle(3)).unwrap();
    for handle in packet_controls.as_slice() {
        controls.push(lane, *handle).unwrap();
    }
    for handle in packet_streams.as_slice() {
        streams.push(lane, *handle).unwrap();
    }
    packets.try_insert(lane, packet).unwrap();

    assert_eq!(packets.remove(lane), Some(packet));
    assert_eq!(controls.drain(lane).collect::<Vec<_>>(), [ControlHandle(1)]);
    assert_eq!(
        streams.drain(lane).collect::<Vec<_>>(),
        [StreamHandle(3), StreamHandle(2)]
    );
    assert_eq!(controls.available(), controls.capacity());
    assert_eq!(streams.available(), streams.capacity());
}

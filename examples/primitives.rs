use std::pin::pin;

use o3::buffer::Pool;
use o3::collections::IndexedMinHeap;
use o3::collections::PinCellSlab;
use o3::collections::Slab;

fn main() {
    let mut connections: Slab<&str> = Slab::with_capacity(64);
    let connection = connections.insert("ready").unwrap();
    assert_eq!(connections.get(connection), Some(&"ready"));

    let mut deadlines = IndexedMinHeap::with_capacity(64);
    deadlines.insert(connection, 10).unwrap();
    assert_eq!(deadlines.pop(), Some((connection, 10)));

    let buffers = pin!(Pool::new(8, 4096));
    let mut buffer = buffers.as_ref().try_acquire().unwrap();
    buffer.try_extend_from_slice(b"response").unwrap();
    assert_eq!(buffer.as_ref(), b"response");

    let fibers = pin!(PinCellSlab::<u32>::with_capacity(64));
    let fiber = fibers.as_ref().insert(1).unwrap();
    let mut entry = fibers.as_ref().entry(fiber).unwrap();
    *entry.get_pin_mut() = 2;
    entry.remove();
}

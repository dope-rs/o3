#![forbid(unsafe_code)]

use o3::buffer::{
    self, BLOCK_CAPACITY, PrefixConsumer,
    pool::{Cursor, FixedCapacity, Pool, state::Uninitialized},
};

struct FixedEgress {
    lease: Cursor<FixedCapacity<BLOCK_CAPACITY>>,
}

impl FixedEgress {
    fn queue(&mut self, prefix: &[u8], payload: &[u8]) {
        self.lease
            .try_extend_from_slices([prefix, payload])
            .expect("the bounded egress admission check must run before encoding");
    }

    fn consume(&mut self, len: usize) {
        self.lease.try_consume_prefix(len).unwrap().commit();
    }

    fn as_slice(&self) -> &[u8] {
        self.lease.as_ref()
    }
}

fn compress_into(
    mut lease: buffer::pool::Lease<buffer::pool::state::Initialized>,
    input: &[u8],
) -> buffer::pool::Frozen {
    let output = &mut lease.spare_mut()[..input.len()];
    for (output, input) in output.iter_mut().zip(input) {
        *output = input.to_ascii_uppercase();
    }
    lease
        .try_advance(input.len())
        .expect("the compression bound must fit the initialized slot");
    lease.freeze()
}

#[test]
fn dope_fixed_egress_reuses_one_compile_time_sized_lease() {
    let pool = Pool::<Uninitialized, FixedCapacity<BLOCK_CAPACITY>>::fixed::<1>();
    let lease = pool.try_acquire_buffer().expect("fixed egress slot");
    let mut egress = FixedEgress { lease };

    egress.queue(b"head:", b"body");
    assert_eq!(egress.as_slice(), b"head:body");

    egress.consume(5);
    egress.queue(b"-", b"tail");
    assert_eq!(egress.as_slice(), b"body-tail");
}

#[test]
fn sark_compression_can_commit_only_initialized_spare_capacity() {
    let pool = buffer::pool::Pool::<buffer::pool::state::Initialized>::try_new(1, 16).unwrap();
    let lease = pool.try_acquire().expect("initialized compression slot");
    let compressed = compress_into(lease, b"body");

    assert_eq!(compressed.as_ref(), b"BODY");
    assert!(pool.try_acquire().is_none());
    drop(compressed);
    assert!(pool.try_acquire().is_some());
}

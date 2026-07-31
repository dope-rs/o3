#![forbid(unsafe_code)]

use o3::buffer::{
    BLOCK_CAPACITY, FixedPoolCapacity, Initialized, Lease, Pool, Pooled, SharedLease, SharedPool,
};

struct FixedEgress<'pool> {
    lease: Lease<'pool, FixedPoolCapacity<BLOCK_CAPACITY>>,
}

impl FixedEgress<'_> {
    fn queue(&mut self, prefix: &[u8], payload: &[u8]) {
        self.lease
            .try_extend_from_slices([prefix, payload])
            .expect("the bounded egress admission check must run before encoding");
    }

    fn consume(&mut self, len: usize) {
        self.lease.try_consume(len).unwrap();
    }

    fn as_slice(&self) -> &[u8] {
        self.lease.as_ref()
    }
}

fn compress_into(mut lease: SharedLease<Initialized>, input: &[u8]) -> Pooled {
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
    let pool = Pool::<FixedPoolCapacity<BLOCK_CAPACITY>>::new(1);
    let lease = pool.try_acquire().expect("fixed egress slot");
    let mut egress = FixedEgress { lease };

    egress.queue(b"head:", b"body");
    assert_eq!(egress.as_slice(), b"head:body");

    egress.consume(5);
    egress.queue(b"-", b"tail");
    assert_eq!(egress.as_slice(), b"body-tail");
    assert_eq!(egress.lease.capacity(), BLOCK_CAPACITY as usize);
}

#[test]
fn sark_compression_can_commit_only_initialized_spare_capacity() {
    let pool = SharedPool::<Initialized>::try_new(1, 16).unwrap();
    let lease = pool.try_acquire().expect("initialized compression slot");
    let compressed = compress_into(lease, b"body");

    assert_eq!(compressed.as_ref(), b"BODY");
    assert!(pool.try_acquire().is_none());
    drop(compressed);
    assert!(pool.try_acquire().is_some());
}

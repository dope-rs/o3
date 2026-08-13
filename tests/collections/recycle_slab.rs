use std::{
    cell::Cell,
    mem::size_of,
    panic::{AssertUnwindSafe, catch_unwind},
};

use o3::collections::slab::{Capacity, recycle};

struct Value {
    bytes: Vec<u8>,
    uses: usize,
}

impl recycle::Recycle for Value {
    type Seed = Vec<u8>;

    fn into_seed(mut self) -> Self::Seed {
        self.bytes.clear();
        self.bytes
    }
}

#[test]
fn leases_are_one_word_and_retain_the_seed() {
    assert_eq!(
        size_of::<recycle::Lease<'static, Value>>(),
        size_of::<usize>()
    );

    let seeds = Cell::new(0);
    let pool = recycle::Pool::with_capacity(Capacity::new(1), || {
        seeds.set(seeds.get() + 1);
        Vec::with_capacity(32)
    });
    assert_eq!(seeds.get(), 1);

    let mut first = pool
        .vacant_entry()
        .expect("first slot")
        .insert_with(|bytes| Value { bytes, uses: 1 });
    let allocation = first.bytes.as_ptr();
    first.bytes.extend_from_slice(b"reused");
    first.uses += 1;
    drop(first);

    let second = pool
        .vacant_entry()
        .expect("recycled slot")
        .insert_with(|bytes| Value { bytes, uses: 3 });
    assert_eq!(second.bytes.as_ptr(), allocation);
    assert!(second.bytes.is_empty());
    assert_eq!(second.uses, 3);
    assert_eq!(seeds.get(), 1);
}

#[test]
fn cancelled_reservation_preserves_the_seed() {
    let pool = recycle::Pool::<Value>::with_capacity(Capacity::new(1), Vec::new);
    drop(pool.vacant_entry().expect("reservation"));
    assert!(pool.vacant_entry().is_some());
}

#[test]
fn reserved_seed_can_be_prepared_before_infallible_insertion() {
    let pool = recycle::Pool::<Value>::with_capacity(Capacity::new(1), Vec::new);
    let mut vacant = pool.vacant_entry().expect("reservation");
    vacant.seed_mut().extend_from_slice(b"prepared");
    let lease = vacant.insert_with(|bytes| Value { bytes, uses: 1 });
    assert_eq!(lease.bytes, b"prepared");
}

#[test]
fn rejected_build_restores_the_returned_seed() {
    let pool = recycle::Pool::<Value>::with_capacity(Capacity::new(1), || Vec::with_capacity(32));
    let allocation = match pool
        .vacant_entry()
        .expect("slot")
        .try_insert_with(|mut bytes| {
            let allocation = bytes.as_ptr();
            bytes.extend_from_slice(b"returned");
            Err((allocation, bytes))
        }) {
        Ok(_) => panic!("rejected build was inserted"),
        Err(allocation) => allocation,
    };

    let value = pool
        .vacant_entry()
        .expect("restored slot")
        .insert_with(|bytes| Value { bytes, uses: 1 });
    assert_eq!(value.bytes.as_ptr(), allocation);
    assert_eq!(value.bytes, b"returned");
}

#[test]
fn zero_capacity_is_permanently_full() {
    let pool = recycle::Pool::<Value>::with_capacity(Capacity::EMPTY, Vec::new);
    assert!(pool.vacant_entry().is_none());
}

struct FailedRecycle;

impl recycle::Recycle for FailedRecycle {
    type Seed = ();

    fn into_seed(self) {
        panic!("recycling failed");
    }
}

#[test]
fn failed_recycling_retires_the_slot() {
    let pool = recycle::Pool::<FailedRecycle>::with_capacity(Capacity::new(1), || ());
    let lease = pool
        .vacant_entry()
        .expect("slot")
        .insert_with(|()| FailedRecycle);

    assert!(catch_unwind(AssertUnwindSafe(|| drop(lease))).is_err());
    assert!(pool.vacant_entry().is_none());
}

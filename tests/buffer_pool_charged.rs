pub mod common;

use common::{alloc::system::Tracker, buffer::domain::Capacity};
use o3::buffer::{pool, raw::Target as Output, resident, storage};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn native_slots_keep_one_root_debit() {
    Capacity(16)
        .try_scope(|budget| {
            let (pool, allocated) = Tracker::measure(|| {
                pool::Pool::try_charged(pool::Layout::constant::<2, 8>(), budget)
            });
            let pool = pool.expect("charged pool");
            assert_eq!(allocated, [1, 72, 0, 0]);
            let activity = Tracker::begin();
            let mut first = pool.acquire().expect("first slot");
            let mut second = pool.acquire().expect("second slot");
            assert!(pool.acquire().is_none());
            // SAFETY: each native-style write initializes its exclusive suffix;
            // all writes end before the corresponding completion.
            unsafe {
                for (target, byte) in first.spare().iter_mut().zip(b"abc") {
                    target.write(*byte);
                }
                first.complete(3);
                first.complete(0);
                second.spare()[0].write(b'd');
                second.complete(1);
            }
            let first = first.freeze();
            let second = second.freeze();
            let returned = second.as_ptr();
            assert_eq!(first.as_slice(), b"abc");
            assert!(
                !first
                    .backing()
                    .unwrap()
                    .contains(&second.backing().unwrap())
            );
            let kept = first.try_clone().expect("immutable alias");
            assert!(first.backing().unwrap().contains(&kept.backing().unwrap()));
            let first = first.try_owned().expect_err("pool remains scoped");
            assert!(first.scope().is_some());
            drop(first);
            drop(second);
            let slot = pool.acquire().expect("returned second slot");
            assert_eq!(slot.as_slice().as_ptr(), returned);
            let empty = slot.freeze();
            assert!(empty.scope().is_none());
            assert_eq!(pool.available(), 1);
            drop(kept);
            assert_eq!(pool.available(), 2);
            assert_eq!(budget.resident(), 16);

            let mut last = pool.acquire().expect("idle pool reuses storage");
            // SAFETY: the one-byte write ends before publishing that byte.
            unsafe {
                last.spare()[0].write(b'z');
                last.complete(1);
            }
            let last = last.freeze();
            drop(pool);
            assert_eq!(last.as_slice(), b"z");
            assert_eq!(budget.resident(), 16);
            assert_eq!(activity.finish().allocations, 0);
            let ((), freed) = Tracker::measure(|| drop(last));
            assert_eq!(freed, [0, 0, 1, 72]);
            assert_eq!(budget.resident(), 0);
        })
        .expect("domain");
}

#[test]
fn root_failures_and_empty_pool_preserve_credit() {
    Capacity(15)
        .try_scope(|budget| {
            let (rejected, activity) = Tracker::measure(|| {
                pool::Pool::try_charged(pool::Layout::constant::<2, 8>(), budget)
            });
            assert!(matches!(rejected, Err(resident::AllocError::Budget(_))));
            assert_eq!(activity, [0; 4]);
            let failed = Tracker::fail_after(0, || {
                pool::Pool::try_charged(pool::Layout::constant::<1, 8>(), budget)
            });
            assert!(matches!(failed, Err(resident::AllocError::Exhausted(_))));
            assert_eq!(budget.resident(), 0);
            let ((), activity) = Tracker::measure(|| {
                let empty = pool::Pool::try_charged(pool::Layout::constant::<0, 1>(), budget)
                    .expect("empty root");
                assert!(empty.acquire().is_none());
                assert_eq!(budget.resident(), 0);
            });
            assert_eq!(activity, [1, 40, 1, 40]);
        })
        .expect("domain");
}

#[test]
fn native_owned_output_publishes_only_completed_bytes() {
    Capacity(8)
        .try_scope(|budget| {
            let mut owned = storage::Owned::try_allocate(&budget, 8).expect("native storage");
            // SAFETY: native writes initialize exactly this suffix before completion.
            unsafe {
                owned.spare()[0].write(b'a');
                owned.complete(1);
                owned.complete(0);
                owned.spare()[0].write(b'b');
                owned.complete(1);
            }
            assert_eq!(owned.as_slice(), b"ab");
            Output::truncate(&mut owned, 1);
            assert_eq!(Output::capacity(&owned), 8);
            assert_eq!(owned.freeze().as_slice(), b"a");
            assert_eq!(budget.resident(), 0);
        })
        .expect("domain");
}

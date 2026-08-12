use o3::buffer::{
    self,
    pool::{self, LayoutError},
};

use crate::confined::assert_confined;

assert_confined!(buffer::Pool);
assert_confined!(buffer::Lease);
assert_confined!(buffer::Pool<pool::state::Initialized>);
assert_confined!(buffer::Lease<pool::state::Initialized>);
assert_confined!(buffer::Frozen);

#[test]
fn frozen_slots_return_after_the_last_clone() {
    let pool = buffer::Pool::<pool::state::Uninitialized>::try_new(1, 16).unwrap();
    let mut lease = pool.try_acquire().unwrap();
    lease.try_extend(b"body").unwrap();
    let body = lease.freeze();
    let clone = body.clone();
    assert!(pool.try_acquire().is_none());
    drop(body);
    assert!(pool.try_acquire().is_none());
    drop(clone);
    assert!(pool.try_acquire().is_some());

    let empty = buffer::Pool::<pool::state::Uninitialized>::try_new(0, 16).unwrap();
    assert_eq!(empty.capacity(), 16);
    assert_eq!(empty.available(), 0);
    assert!(empty.try_acquire().is_none());
}

#[test]
fn frozen_slot_outlives_the_pool_handle() {
    let body = {
        let pool = buffer::Pool::<pool::state::Uninitialized>::try_new(1, 8).unwrap();
        let mut lease = pool.try_acquire().unwrap();
        lease.try_extend(b"abc").unwrap();
        assert_eq!(lease.len(), 3);
        assert_eq!(lease.as_slice(), b"abc");
        lease.freeze()
    };
    assert_eq!(body.as_ref(), b"abc");
}

#[test]
fn invalid_layout_is_reported_before_allocation() {
    assert!(matches!(
        buffer::Pool::<pool::state::Uninitialized>::try_new(1, 0),
        Err(LayoutError::ZeroCapacity)
    ));
    assert!(matches!(
        buffer::Pool::<pool::state::Uninitialized>::try_new(usize::MAX, 1),
        Err(LayoutError::SlotOverflow)
    ));
    assert!(matches!(
        buffer::Pool::<pool::state::Uninitialized>::try_new(u32::MAX as usize, u32::MAX as usize,),
        Err(LayoutError::CapacityOverflow)
    ));
}

#[test]
fn validated_layout_constructs_multiple_infallible_pool_instances() {
    let layout = buffer::Layout::new(2, 32).unwrap();
    assert_eq!(layout.slots(), 2);

    let first = buffer::Pool::<pool::state::Uninitialized>::from_layout(layout);
    let second = buffer::Pool::<pool::state::Initialized>::from_layout(layout);
    assert_eq!(first.available(), 2);
    assert_eq!(second.available(), 2);
}

#[test]
fn validated_layout_can_report_allocation_failure() {
    let layout = buffer::Layout::new(1, 8).unwrap();
    let pool = buffer::Pool::<pool::state::Uninitialized>::try_from_layout(layout).unwrap();
    assert_eq!(pool.capacity(), 8);
    assert_eq!(pool.available(), 1);
}

#[test]
fn fixed_plan_proves_every_smaller_layout() {
    let plan = buffer::Plan::fixed::<8, 256>();
    let full = plan.layout_up_to(usize::MAX);
    assert_eq!(full.slots(), 8);
    assert_eq!(plan.layout_up_to(3).slots(), 3);
}

#[test]
fn initialized_slots_expose_spare_capacity_without_clearing_on_reuse() {
    assert_eq!(
        std::mem::size_of::<buffer::Pool<pool::state::Initialized>>(),
        std::mem::size_of::<buffer::Pool>(),
    );
    assert_eq!(
        std::mem::size_of::<buffer::Lease<pool::state::Initialized>>(),
        std::mem::size_of::<buffer::Lease>(),
    );

    let pool = buffer::Pool::<pool::state::Initialized>::try_new(1, 8).unwrap();
    let mut lease = pool.try_acquire().expect("initialized slot");
    assert_eq!(lease.spare_mut(), &[0; 8]);
    lease.spare_mut()[..4].copy_from_slice(b"body");
    lease.try_advance(4).expect("slot capacity");
    assert_eq!(lease.as_slice(), b"body");
    drop(lease);

    let mut reused = pool.try_acquire().expect("returned initialized slot");
    assert_eq!(&reused.spare_mut()[..4], b"body");
    reused.spare_mut()[..3].copy_from_slice(b"new");
    reused.try_advance(3).expect("slot capacity");
    assert_eq!(reused.freeze().as_ref(), b"new");
}

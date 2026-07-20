use o3::buffer::{OwnedView, Shared, SharedPool, View};

#[cfg(target_pointer_width = "64")]
#[test]
fn owned_view_stays_compact() {
    assert_eq!(std::mem::size_of::<OwnedView>(), 32);
}

#[test]
fn view_preserves_borrowed_and_shared_ranges() {
    let borrowed = View::from_slice(b"abcdef").slice(1..5).slice(1..3);
    assert_eq!(borrowed.as_slice(), b"cd");
    assert_eq!(borrowed.into_shared().as_slice(), b"cd");

    let shared = Shared::copy_from_slice(b"012345");
    let view = View::from_shared_range(shared.clone(), 1..5).slice(1..3);
    assert_eq!(view.as_slice(), b"23");
    assert_eq!(view.into_shared().as_slice(), b"23");
    assert_eq!(shared.as_slice(), b"012345");

    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        shared.slice((
            std::ops::Bound::Excluded(usize::MAX),
            std::ops::Bound::Unbounded,
        ))
    }));
    assert!(caught.is_err());
    let caught =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| shared.slice(..=usize::MAX)));
    assert!(caught.is_err());
}

#[test]
fn owned_view_retains_pooled_storage() {
    let pool = SharedPool::new(1, 8);
    let mut lease = pool.try_acquire().unwrap();
    lease.spare_writer().try_extend_from_slice(b"abcd").unwrap();
    let pooled = lease.freeze();
    let ptr = pooled.as_slice().as_ptr();
    let mut view = View::from_pooled(pooled).into_owned();

    assert_eq!(view.as_slice().as_ptr(), ptr);
    assert_eq!(pool.available(), 0);
    view.advance(1);
    assert_eq!(view.as_slice(), b"bcd");
    assert_eq!(view.as_slice().as_ptr(), ptr.wrapping_add(1));

    drop(view);
    assert_eq!(pool.available(), 1);
}

#[test]
fn owned_view_slices_without_copying() {
    let pool = SharedPool::new(1, 8);
    let mut lease = pool.try_acquire().unwrap();
    lease
        .spare_writer()
        .try_extend_from_slice(b"abcdef")
        .unwrap();
    let ptr = lease.as_slice().as_ptr();
    let view = View::from_pooled(lease.freeze()).into_owned().slice(2..5);

    assert_eq!(view.as_slice(), b"cde");
    assert_eq!(view.as_slice().as_ptr(), ptr.wrapping_add(2));
    assert_eq!(pool.available(), 0);
    drop(view);
    assert_eq!(pool.available(), 1);
}

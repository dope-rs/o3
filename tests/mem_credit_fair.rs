use o3::mem::{credit::fair, raw::Balance};

#[test]
fn computes_waiting_demand() {
    let balance = Balance::new([1, u32::MAX, 1], [0; 3]);
    assert_eq!(
        balance.demand([u32::MAX, u32::MAX, 2], [u32::MAX, u32::MAX, 4]),
        [1, u32::MAX, 3],
    );
}

#[test]
fn protects_and_shares() {
    let (first, shared) = Balance::new([0u32], [4])
        .try_acquire([2], [3])
        .unwrap()
        .try_acquire([2], [3])
        .unwrap()
        .into_parts();
    let (second, shared) = Balance::new([0], shared)
        .try_acquire([2], [2])
        .unwrap()
        .into_parts();
    assert_eq!((first, second, shared), ([6], [2], [0]));

    // SAFETY: this returns the first lane's original three-unit acquisition.
    let (first, shared) = unsafe { Balance::new(first, shared).release_raw([2], [3]) }.into_parts();
    let (second, shared) = Balance::new(second, shared)
        .try_acquire([2], [1])
        .unwrap()
        .into_parts();
    assert_eq!((first, second, shared), ([3], [3], [2]));
    assert_eq!(
        Balance::new(first, shared)
            .try_acquire([2], [9])
            .unwrap_err(),
        fair::Cause::Exhausted {
            axis: fair::Axis::at::<0>(),
            required: 9,
            available: 2,
        }
    );

    // SAFETY: each lane returns its remaining disjoint acquisitions exactly once.
    let (first, shared) = unsafe { Balance::new(first, shared).release_raw([2], [3]) }.into_parts();
    let (second, shared) =
        unsafe { Balance::new(second, shared).release_raw([2], [3]) }.into_parts();
    assert_eq!((first, second, shared), ([0], [0], [4]));
}

#[test]
fn uses_full_width() {
    let balance = Balance::new([0usize], [usize::MAX])
        .try_acquire([0], [usize::MAX])
        .unwrap();
    assert_eq!(balance.into_parts(), ([usize::MAX], [0]));
    assert_eq!(
        balance.try_acquire([0], [1]).unwrap_err(),
        fair::Cause::Overflow {
            axis: fair::Axis::at::<0>(),
            held: usize::MAX,
            requested: 1,
        }
    );
}

#[test]
fn rejects_late_axis() {
    let balance = Balance::new([0u32; 2], [4, 40]);
    assert_eq!(
        balance.try_acquire([2, 20], [6, 61]).unwrap_err(),
        fair::Cause::Exhausted {
            axis: fair::Axis::at::<1>(),
            required: 41,
            available: 40,
        }
    );
    assert_eq!(balance.into_parts(), ([0, 0], [4, 40]));
    let acquired = balance.try_acquire([2, 20], [6, 60]).unwrap();
    assert_eq!(acquired.into_parts(), ([6, 60], [0, 0]));
    // SAFETY: the unchanged acquired value returns its exact two-axis acquisition.
    let returned = unsafe { acquired.release_raw([2, 20], [6, 60]) };
    assert_eq!(returned.into_parts(), balance.into_parts());
}

#[test]
fn overflows_late_axis() {
    let balance = Balance::new([0u32; 2], [u32::MAX; 2])
        .try_acquire([0; 2], [u32::MAX; 2])
        .unwrap();
    assert_eq!(
        balance.try_acquire([0; 2], [0, 1]).unwrap_err(),
        fair::Cause::Overflow {
            axis: fair::Axis::at::<1>(),
            held: u32::MAX,
            requested: 1,
        }
    );
    assert_eq!(balance.into_parts(), ([u32::MAX; 2], [0, 0]));
}

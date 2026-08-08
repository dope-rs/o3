use o3::buffer::{
    BLOCK_CAPACITY, CapacityError,
    storage::{Owned, shared::Shared},
    view::Snapshot,
};

type FixedOwned = Owned<BLOCK_CAPACITY>;
const FIXED_CAPACITY: usize = BLOCK_CAPACITY as usize;

#[test]
fn buffer_handles_stay_thin() {
    if usize::BITS == 64 {
        assert_eq!(size_of::<Owned>(), 16);
        assert_eq!(size_of::<FixedOwned>(), 16);
        assert_eq!(size_of::<Shared>(), 24);
        assert_eq!(size_of::<Snapshot<1_048_576>>(), 24);
    }
}

#[test]
fn capacity_policies_preserve_public_identity() {
    let owned = Owned::try_with_capacity(5).unwrap();
    let block = FixedOwned::new();

    assert_eq!(format!("{owned:?}"), "Owned { len: 0, capacity: 5 }");
    assert_eq!(format!("{block:?}"), "Owned { len: 0, capacity: 65536 }");
}

#[test]
fn clone_copies_and_freeze_transfers_the_fixed_allocation() {
    let mut owned = FixedOwned::new();
    owned
        .try_extend(b"fixed block")
        .expect("payload must fit the fixed block");

    let clone = owned.clone();
    assert_eq!(clone.as_slice(), owned.as_slice());
    assert_ne!(clone.as_ptr(), owned.as_ptr());

    let ptr = owned.as_ptr();
    let shared = owned.freeze();
    assert_eq!(shared.as_ptr(), ptr);
    assert_eq!(shared.as_slice(), b"fixed block");
    assert_eq!(shared.clone().as_slice(), b"fixed block");
}

#[test]
fn large_vec_transfers_and_shares_its_allocation() {
    let payload = vec![b'x'; 4096];
    let ptr = payload.as_ptr();
    let shared = Shared::from(payload);
    assert_eq!(shared.as_ptr(), ptr);

    let slice = shared.get(1024..3072).unwrap();
    drop(shared);
    assert_eq!(slice.as_slice(), &[b'x'; 2048]);
    assert_eq!(slice.as_ptr(), ptr.wrapping_add(1024));
}

#[test]
fn spare_writer_commits_checked_storage() {
    let mut owned = FixedOwned::new();
    let mut spare = owned.spare_writer();
    spare
        .try_extend(b"xyz")
        .expect("small write must fit the fixed block");
    spare.finish();
    assert_eq!(owned.as_slice(), b"xyz");

    let mut writer = owned.spare_writer();
    writer.try_extend(b"raw").unwrap();
    assert_eq!(writer.finish(), 3);
    assert_eq!(owned.as_slice(), b"xyzraw");
}

#[test]
fn fixed_capacity_accepts_exactly_its_capacity() {
    let bytes = vec![b'x'; FIXED_CAPACITY];
    let mut owned = FixedOwned::new();
    owned
        .try_extend(&bytes)
        .expect("one complete block must fit");
    assert_eq!(owned.len(), FIXED_CAPACITY);

    let error = owned
        .try_push(b'y')
        .expect_err("a full block must reject another byte");
    assert_eq!(
        error.to_string(),
        format!(
            "capacity exceeded: attempted {}, capacity {}",
            FIXED_CAPACITY + 1,
            FIXED_CAPACITY
        )
    );
    assert_eq!(owned.as_slice(), bytes);
}

#[test]
fn oversized_write_leaves_the_fixed_owner_unchanged() {
    let mut owned = FixedOwned::new();
    owned.try_extend(b"prefix").expect("prefix must fit");
    let oversized = vec![0; FIXED_CAPACITY];

    let error = owned
        .try_extend(&oversized)
        .expect_err("combined payload must exceed the block");
    assert_eq!(
        error.to_string(),
        format!(
            "capacity exceeded: attempted {}, capacity {}",
            FIXED_CAPACITY + b"prefix".len(),
            FIXED_CAPACITY
        )
    );
    assert_eq!(owned.as_slice(), b"prefix");
}

#[test]
fn owned_has_an_exact_runtime_capacity_without_growth() {
    let mut owned = Owned::try_with_capacity(5).unwrap();
    assert_eq!(owned.capacity(), 5);
    owned
        .try_extend(b"exact")
        .expect("the exact payload must fit");
    assert_eq!(owned.as_slice(), b"exact");
    assert!(owned.try_push(b'!').is_err());
}

#[test]
fn owned_fills_its_exact_allocation() {
    let owned = Owned::try_filled(4, b'x').unwrap();
    assert_eq!(owned.capacity(), 4);
    assert_eq!(owned.as_slice(), b"xxxx");
}

#[test]
fn exact_build_keeps_initialization_inside_the_safe_writer() {
    let owned = Owned::try_build_exact(4, |out| {
        for byte in b"safe" {
            out.try_push(*byte)?;
        }
        Ok::<_, CapacityError>(())
    })
    .expect("checked byte writes fill the exact allocation");

    assert_eq!(owned.as_slice(), b"safe");
}

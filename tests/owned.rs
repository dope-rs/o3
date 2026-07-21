use o3::buffer::{Block, Owned, Shared, SnapshotBuf};

#[cfg(target_pointer_width = "64")]
#[test]
fn buffer_handles_stay_thin() {
    assert_eq!(size_of::<Owned>(), 16);
    assert_eq!(size_of::<Block>(), 16);
    assert_eq!(size_of::<Shared>(), 24);
    assert_eq!(size_of::<SnapshotBuf<1_048_576>>(), 24);
}

#[test]
fn block_is_one_reusable_fixed_allocation() {
    assert_eq!(Block::CAPACITY, 64 * 1024);

    let mut owned = Block::new();
    assert!(owned.is_empty());
    let ptr = owned.as_ptr();

    owned
        .try_extend_from_slice(b"hello")
        .expect("small write must fit the fixed block");
    owned
        .try_push(b'!')
        .expect("single byte must fit the fixed block");
    assert_eq!(owned.as_slice(), b"hello!");

    owned.truncate(5);
    assert_eq!(owned.as_slice(), b"hello");
    owned.clear();
    assert!(owned.is_empty());
    assert_eq!(owned.as_ptr(), ptr);

    owned
        .try_extend_from_slice(b"reused")
        .expect("clearing must preserve reusable storage");
    assert_eq!(owned.as_slice(), b"reused");
}

#[test]
fn clone_copies_and_freeze_transfers_the_fixed_block() {
    let mut owned = Block::new();
    owned
        .try_extend_from_slice(b"fixed block")
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

    let slice = shared.slice(1024..3072);
    drop(shared);
    assert_eq!(slice.as_slice(), &[b'x'; 2048]);
    assert_eq!(slice.as_ptr(), ptr.wrapping_add(1024));
}

#[test]
fn spare_writer_commits_initialized_storage() {
    let mut owned = Block::new();
    let mut spare = owned.spare_writer();
    assert_eq!(spare.capacity(), Block::CAPACITY);
    spare
        .try_extend_from_slice(b"xyz")
        .expect("small write must fit the fixed block");
    spare.finish();
    assert_eq!(owned.as_slice(), b"xyz");

    let mut writer = owned.spare_writer();
    let ptr = writer.as_mut_ptr();
    unsafe { std::ptr::copy_nonoverlapping(b"raw".as_ptr(), ptr, 3) };
    let initialized = unsafe { std::slice::from_raw_parts(ptr, 3) };
    writer
        .try_commit_initialized(initialized)
        .expect("initialized bytes came from this writer");
    assert_eq!(writer.finish(), 3);
    assert_eq!(owned.as_slice(), b"xyzraw");
}

#[test]
fn fixed_capacity_accepts_exactly_one_block() {
    let bytes = vec![b'x'; Block::CAPACITY];
    let mut owned = Block::new();
    owned
        .try_extend_from_slice(&bytes)
        .expect("one complete block must fit");
    assert_eq!(owned.len(), Block::CAPACITY);

    let error = owned
        .try_push(b'y')
        .expect_err("a full block must reject another byte");
    assert_eq!(error.attempted(), Block::CAPACITY + 1);
    assert_eq!(error.capacity(), Block::CAPACITY);
    assert_eq!(owned.as_slice(), bytes);
}

#[test]
fn oversized_write_leaves_the_block_unchanged() {
    let mut owned = Block::new();
    owned
        .try_extend_from_slice(b"prefix")
        .expect("prefix must fit");
    let oversized = vec![0; Block::CAPACITY];

    let error = owned
        .try_extend_from_slice(&oversized)
        .expect_err("combined payload must exceed the block");
    assert_eq!(error.attempted(), Block::CAPACITY + b"prefix".len());
    assert_eq!(error.capacity(), Block::CAPACITY);
    assert_eq!(owned.as_slice(), b"prefix");
}

#[test]
fn owned_has_an_exact_runtime_capacity_without_growth() {
    let mut owned = Owned::with_capacity(5);
    assert_eq!(owned.capacity(), 5);
    owned
        .try_extend_from_slice(b"exact")
        .expect("the exact payload must fit");
    assert_eq!(owned.as_slice(), b"exact");
    assert!(owned.try_push(b'!').is_err());
}

#[test]
fn owned_fills_its_exact_allocation() {
    let owned = Owned::filled(4, b'x');
    assert_eq!(owned.capacity(), 4);
    assert_eq!(owned.as_slice(), b"xxxx");
}

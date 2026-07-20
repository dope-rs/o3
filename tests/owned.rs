use o3::buffer::{Owned, Shared};

#[cfg(target_pointer_width = "64")]
#[test]
fn buffer_handles_stay_thin() {
    assert_eq!(size_of::<Owned>(), 24);
    assert_eq!(size_of::<Shared>(), 24);
}

#[test]
fn vector_backed_storage_preserves_allocation_and_bytes() {
    let owned_vec = b"abc".to_vec();
    let owned_ptr = owned_vec.as_ptr();
    let mut owned = Owned::from(owned_vec);
    let shared_vec = b"def".to_vec();
    let shared_ptr = shared_vec.as_ptr();
    let shared = Shared::from(shared_vec);
    assert_eq!(owned.as_slice(), b"abc");
    assert_eq!(shared.as_slice(), b"def");
    assert_eq!(owned.as_ptr(), owned_ptr);
    assert_eq!(shared.as_ptr(), shared_ptr);
    owned.reserve(32);
    owned.extend_from_slice(b"45678");
    let head = owned.split_to(3);
    let tail = owned.split_off(2);
    assert_eq!(head.as_slice(), b"abc");
    assert_eq!(owned.as_slice(), b"45");
    assert_eq!(tail.as_slice(), b"678");
}

#[test]
fn native_storage_freezes_and_splits_without_copying() {
    let mut frozen = Owned::new();
    assert!(frozen.is_empty());
    assert_eq!(frozen.capacity(), 0);
    frozen.reserve(0);
    frozen.extend_from_slice(b"hello world");
    let frozen_ptr = frozen.as_ptr();
    let frozen = frozen.freeze();
    assert_eq!(frozen.as_ptr(), frozen_ptr);
    assert_eq!(frozen.as_slice(), b"hello world");
    assert_eq!(frozen.clone().as_slice(), b"hello world");

    let mut value = Owned::with_capacity(64);
    value.extend_from_slice(b"abcdefgh");
    let head = value.split_to(3);
    assert_eq!(head.capacity(), 64);
    assert_eq!(value.capacity(), 5);
    assert_eq!(head.as_slice(), b"abc");
    assert_eq!(value.as_slice(), b"defgh");
    let tail = value.split_off(2);
    assert_eq!(value.as_slice(), b"de");
    assert_eq!(tail.as_slice(), b"fgh");

    let mut reusable = Owned::with_capacity(16);
    reusable.extend_from_slice(b"first");
    let split_ptr = reusable.as_ptr();
    let first: Shared = reusable.split();
    assert_eq!(first.as_ptr(), split_ptr);
    assert_eq!(first.as_slice(), b"first");
    assert!(reusable.is_empty());

    reusable.extend_from_slice(b"second");
    let second: Shared = reusable.freeze();
    assert_eq!(second.as_slice(), b"second");
}

#[test]
fn spare_writer_commits_initialized_storage() {
    let mut o = Owned::with_capacity(8);
    let mut spare = o.spare_writer();
    assert_eq!(spare.capacity(), 8);
    spare.try_extend_from_slice(b"xyz").unwrap();
    spare.finish();
    assert_eq!(o.as_slice(), b"xyz");

    let mut owned = Owned::with_capacity(8);
    let mut writer = owned.spare_writer();
    let ptr = writer.as_mut_ptr();
    unsafe { std::ptr::copy_nonoverlapping(b"raw".as_ptr(), ptr, 3) };
    let initialized = unsafe { std::slice::from_raw_parts(ptr, 3) };
    writer.try_commit_initialized(initialized).unwrap();
    assert_eq!(writer.finish(), 3);
    assert_eq!(owned.as_slice(), b"raw");
}

#[test]
fn writer_commits_initialized_bytes() {
    let mut owned = Owned::copy_from_slice(b"head");
    let mut writer = owned.writer(8);
    writer.extend_from_slice(b" body");
    writer.push(b'!');
    assert_eq!(writer.finish(), 6);
    assert_eq!(owned.as_slice(), b"head body!");
}

use o3::buffer::{BLOCK_CAPACITY, Owned};

#[test]
fn exact_and_fixed_owners_share_operations_without_sharing_capacity_types() {
    let payload = b"response bytes";
    let mut exact = Owned::try_with_capacity(payload.len()).unwrap();
    let mut block = Owned::<BLOCK_CAPACITY>::new();

    exact.try_extend(payload).unwrap();
    block.try_extend(payload).unwrap();

    assert_eq!(exact, block);
    assert_eq!(exact.capacity(), payload.len());
    assert_eq!(block.capacity(), BLOCK_CAPACITY as usize);

    let exact_ptr = exact.as_ptr();
    let block_ptr = block.as_ptr();
    let exact = exact.freeze();
    let block = block.freeze();

    assert_eq!(exact.as_ptr(), exact_ptr);
    assert_eq!(block.as_ptr(), block_ptr);
    assert_eq!(exact, block);
}

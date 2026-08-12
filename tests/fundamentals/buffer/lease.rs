use o3::buffer::{self, PrefixConsumer};

#[test]
fn shared_lease_cursor_keeps_the_former_wrapper_layout() {
    if usize::BITS == 64 {
        assert_eq!(size_of::<buffer::pool::Cursor>(), 3 * size_of::<usize>());
    }
}

#[test]
fn shared_lease_buffer_compacts_only_for_an_admissible_append() {
    let pool = buffer::Pool::try_new(1, 16).unwrap();
    let mut buffer = pool.try_acquire_buffer().unwrap();
    buffer.try_extend(b"discardpayload").unwrap();
    buffer.try_consume_prefix(7).unwrap().commit();

    buffer.try_extend(b"tail").unwrap();

    assert_eq!(buffer.as_slice(), b"payloadtail");
}

#[test]
fn freezing_preserves_only_the_unconsumed_range_without_copying() {
    let pool = buffer::Pool::try_new(1, 16).unwrap();
    let mut buffer = pool.try_acquire_buffer().unwrap();
    buffer.try_extend(b"discardpayload").unwrap();
    let expected = buffer.as_slice()[7..].as_ptr();
    buffer.try_consume_prefix(7).unwrap().commit();

    let frozen = buffer.freeze();

    assert_eq!(frozen.as_slice(), b"payload");
    assert_eq!(frozen.as_slice().as_ptr(), expected);
    assert_eq!(pool.available(), 0);
    drop(frozen);
    assert_eq!(pool.available(), 1);
}

#[test]
fn exact_transaction_rolls_back_until_it_is_complete() {
    let pool = buffer::Pool::try_new(1, 16).unwrap();
    let mut buffer = pool.try_acquire_buffer().unwrap();
    let mut writer = buffer.spare_writer();
    {
        let mut transaction = writer.try_transaction(4).unwrap();
        transaction.try_extend(b"two").unwrap();
        assert!(transaction.commit().is_err());
    }
    assert_eq!(writer.len(), 0);

    let mut transaction = writer.try_transaction(4).unwrap();
    transaction.try_extend(b"tw").unwrap();
    transaction.try_extend(b"o!").unwrap();
    transaction.commit().unwrap();
    drop(writer);

    assert_eq!(buffer.as_slice(), b"two!");
}

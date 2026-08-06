use o3::buffer;

#[test]
fn bounded_protocol_output_commits_only_a_complete_safe_record() {
    let pool = buffer::Pool::try_new(1, 32).unwrap();
    let mut output = pool.try_acquire_buffer().unwrap();

    {
        let mut writer = output.spare_writer();
        let mut record = writer.try_transaction(7).unwrap();
        record.try_extend(b"kind:").unwrap();
        record.try_extend(b"ok").unwrap();
        record.commit().unwrap();
    }
    assert_eq!(output.as_slice(), b"kind:ok");

    {
        let mut writer = output.spare_writer();
        let mut incomplete = writer.try_transaction(5).unwrap();
        incomplete.try_extend(b"bad").unwrap();
    }
    assert_eq!(output.as_slice(), b"kind:ok");
}

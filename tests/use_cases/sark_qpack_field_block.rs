#![forbid(unsafe_code)]

use o3::buffer::{CapacityError, Owned, SpareWriter};

fn push_field(
    writer: &mut SpareWriter<'_>,
    name: &[u8],
    value: &[u8],
) -> Result<(), CapacityError> {
    let start = writer.len();
    let written = (|| {
        writer.try_extend_from_slice(&[0; 8])?;
        writer.try_extend_from_slice(name)?;
        writer.try_extend_from_slice(value)?;
        Ok::<_, CapacityError>(())
    })();
    if let Err(error) = written {
        writer.truncate(start);
        return Err(error);
    }

    let name_len = u32::try_from(name.len()).expect("test field name fits the packed format");
    let value_len = u32::try_from(value.len()).expect("test field value fits the packed format");
    let field = &mut writer.as_mut_slice()[start..];
    field[..4].copy_from_slice(&name_len.to_ne_bytes());
    field[4..8].copy_from_slice(&value_len.to_ne_bytes());
    Ok(())
}

fn read_field<'a>(packed: &'a [u8], cursor: &mut usize) -> (&'a [u8], &'a [u8]) {
    let name_len = u32::from_ne_bytes(packed[*cursor..*cursor + 4].try_into().unwrap()) as usize;
    let value_len =
        u32::from_ne_bytes(packed[*cursor + 4..*cursor + 8].try_into().unwrap()) as usize;
    let name_start = *cursor + 8;
    let value_start = name_start + name_len;
    *cursor = value_start + value_len;
    (
        &packed[name_start..value_start],
        &packed[value_start..*cursor],
    )
}

#[test]
fn sark_qpack_decodes_one_field_block_with_backfill_and_rollback() {
    let mut block = Owned::try_with_capacity(64).expect("bounded QPACK field block");
    let mut writer = block.spare_writer();

    push_field(&mut writer, b":method", b"GET").unwrap();
    assert!(push_field(&mut writer, b"x-oversized", &[b'x'; 64]).is_err());
    push_field(&mut writer, b":path", b"/").unwrap();
    writer.finish();

    let mut cursor = 0;
    assert_eq!(
        read_field(block.as_slice(), &mut cursor),
        (b":method".as_slice(), b"GET".as_slice())
    );
    assert_eq!(
        read_field(block.as_slice(), &mut cursor),
        (b":path".as_slice(), b"/".as_slice())
    );
    assert_eq!(cursor, block.len());
}

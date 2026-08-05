#![forbid(unsafe_code)]

use o3::buffer::{CapacityError, ExactBuildError, Owned, SpareWriter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct JsonEscapeError;

fn decode_sark_json_escapes(
    raw: &[u8],
    decoded_len: usize,
) -> Result<Owned, ExactBuildError<JsonEscapeError>> {
    Owned::try_build_exact(decoded_len, |out| decode_into(raw, out))
}

fn decode_into(raw: &[u8], out: &mut SpareWriter<'_>) -> Result<(), JsonEscapeError> {
    let mut cursor = 0;
    while let Some(&byte) = raw.get(cursor) {
        cursor += 1;
        if byte != b'\\' {
            push(out, byte)?;
            continue;
        }
        let escaped = *raw.get(cursor).ok_or(JsonEscapeError)?;
        cursor += 1;
        let decoded = match escaped {
            b'"' | b'\\' | b'/' => escaped,
            b'b' => 0x08,
            b'f' => 0x0c,
            b'n' => b'\n',
            b'r' => b'\r',
            b't' => b'\t',
            _ => return Err(JsonEscapeError),
        };
        push(out, decoded)?;
    }
    Ok(())
}

fn push(out: &mut SpareWriter<'_>, byte: u8) -> Result<(), JsonEscapeError> {
    out.try_push(byte)
        .map_err(|_: CapacityError| JsonEscapeError)
}

#[test]
fn sark_json_escape_decode_fills_one_exact_owned_buffer_without_raw_commit() {
    let raw = br#"a\"b\\c\/d\b\f\n\r\t"#;
    let expected = b"a\"b\\c/d\x08\x0c\n\r\t";

    let owned = decode_sark_json_escapes(raw, expected.len())
        .expect("the exact decoded length admits every checked byte write");

    assert_eq!(owned.len(), expected.len());
    assert_eq!(owned.capacity(), expected.len());
    assert_eq!(owned.as_slice(), expected);
}

#[test]
fn sark_json_escape_decode_rejects_an_inaccurate_length_without_exposing_spare_storage() {
    let error = decode_sark_json_escapes(br#"a\"b"#, 4)
        .expect_err("the length pass must account for decoded escapes, not source bytes");

    assert_eq!(
        error,
        ExactBuildError::LengthMismatch {
            expected: 4,
            actual: 3,
        }
    );
}

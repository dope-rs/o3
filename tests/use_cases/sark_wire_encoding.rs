#![forbid(unsafe_code)]

use o3::buffer::{
    CapacityError,
    storage::{BuildError, Owned, Shared},
    write::SpareWriter,
};

struct Response<'a> {
    static_head: &'static [u8],
    fields: &'a [(&'a [u8], &'a [u8])],
    body: &'a [u8],
}

impl Response<'_> {
    fn encoded_len(&self) -> usize {
        self.static_head.len()
            + self
                .fields
                .iter()
                .map(|(name, value)| name.len() + b": ".len() + value.len() + b"\r\n".len())
                .sum::<usize>()
            + b"\r\n".len()
            + self.body.len()
    }

    fn encode_into(&self, out: &mut SpareWriter<'_>) -> Result<(), CapacityError> {
        out.try_extend(self.static_head)?;
        for (name, value) in self.fields {
            out.try_extend(name)?;
            out.try_extend(b": ")?;
            out.try_extend(value)?;
            out.try_extend(b"\r\n")?;
        }
        out.try_extend(b"\r\n")?;
        out.try_extend(self.body)
    }

    fn into_shared(self) -> Result<Shared, BuildError<CapacityError>> {
        let encoded_len = self.encoded_len();
        Owned::try_build_exact(encoded_len, |out| self.encode_into(out)).map(Owned::freeze)
    }
}

#[test]
fn sark_length_first_wire_encoding_uses_one_exact_allocation() {
    let response = Response {
        static_head: b"HTTP/1.1 200 OK\r\n",
        fields: &[
            (b"content-type", b"application/json"),
            (b"content-length", b"11"),
        ],
        body: br#"{"ok":true}"#,
    };
    let expected_len = response.encoded_len();

    let owned = Owned::try_build_exact(expected_len, |out| response.encode_into(out))
        .expect("the length pass and encoding pass must agree");
    assert_eq!(owned.capacity(), expected_len);
    assert_eq!(owned.len(), expected_len);

    let allocation = owned.as_ptr();
    let shared = owned.freeze();
    assert_eq!(shared.as_ptr(), allocation);
    assert_eq!(
        shared.as_slice(),
        b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}"
    );
}

#[test]
fn sark_response_conversion_keeps_the_length_contract_at_the_use_case_boundary() {
    let response = Response {
        static_head: b"HTTP/1.1 204 No Content\r\n",
        fields: &[],
        body: b"",
    };

    let shared = response
        .into_shared()
        .expect("the response must fill its exact wire allocation");
    assert_eq!(shared, b"HTTP/1.1 204 No Content\r\n\r\n".as_slice());
}

#[test]
fn sark_inaccurate_length_pass_cannot_produce_a_partially_initialized_buffer() {
    let error = Owned::try_build_exact(5, |out| {
        out.try_extend(b"four")?;
        Ok::<_, CapacityError>(())
    })
    .expect_err("an underfilled exact allocation must be rejected");

    assert_eq!(
        error,
        BuildError::LengthMismatch {
            expected: 5,
            actual: 4,
        }
    );
}

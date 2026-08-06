use o3::buffer::{
    PrefixConsumer, PrefixLength, PrefixProof, ValidatedPrefix, view::window::Inline,
};

struct Cursor {
    bytes: Vec<u8>,
    head: usize,
}

impl PrefixLength for Cursor {
    fn prefix_len(&self) -> usize {
        self.bytes.len() - self.head
    }
}

impl PrefixConsumer for Cursor {
    fn consume_validated_prefix(&mut self, proof: PrefixProof) {
        self.head += proof.amount();
    }
}

#[test]
fn validation_is_bound_to_the_exclusive_target() {
    let mut cursor = Cursor {
        bytes: b"abcdef".to_vec(),
        head: 0,
    };
    let prefix = cursor.try_consume_prefix(4).unwrap();
    prefix.commit();
    assert_eq!(&cursor.bytes[cursor.head..], b"ef");
}

#[test]
fn invalid_prefixes_do_not_consume_the_target() {
    let mut cursor = Cursor {
        bytes: b"abc".to_vec(),
        head: 0,
    };
    let error = match cursor.try_consume_prefix(4) {
        Ok(_) => panic!("oversized prefix was accepted"),
        Err(error) => error,
    };
    assert_eq!(
        error.to_string(),
        "capacity exceeded: attempted 4, capacity 3"
    );
    assert_eq!(cursor.head, 0);
}

#[test]
fn bounded_prefix_consumption_uses_the_consumer_contract() {
    let mut cursor = Cursor {
        bytes: b"abcdef".to_vec(),
        head: 0,
    };
    assert_eq!(cursor.consume_prefix_up_to(3), 3);
    cursor.try_consume_prefix(3).unwrap().commit();
    assert_eq!(cursor.head, 6);
}

#[test]
fn native_buffer_prefix_commit_has_no_second_validation_surface() {
    let mut buffer = Inline::<16>::default();
    buffer.try_extend(b"abcdef").unwrap();
    buffer.try_consume_prefix(4).unwrap().commit();
    assert_eq!(buffer.as_slice(), b"ef");
    assert_eq!(buffer.consume_prefix_up_to(8), 2);
    assert!(buffer.is_empty());
}

#[cfg(target_pointer_width = "64")]
#[test]
fn prefix_proof_keeps_only_the_exclusive_target_and_length() {
    assert_eq!(std::mem::size_of::<ValidatedPrefix<'_, Inline<16>>>(), 16);
}

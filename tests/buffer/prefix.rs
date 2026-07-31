use o3::buffer::{PrefixLength, RollingBuffer, ValidatedPrefix};

struct Cursor {
    bytes: Vec<u8>,
    head: usize,
}

impl PrefixLength for Cursor {
    fn prefix_len(&self) -> usize {
        self.bytes.len() - self.head
    }
}

fn commit(cursor: &mut Cursor, amount: usize) {
    cursor.head += amount;
}

#[test]
fn validation_is_bound_to_the_exclusive_target() {
    let mut cursor = Cursor {
        bytes: b"abcdef".to_vec(),
        head: 0,
    };
    let prefix = ValidatedPrefix::try_new(&mut cursor, 4, commit).unwrap();
    assert_eq!(prefix.len(), 4);
    prefix.commit();
    assert_eq!(&cursor.bytes[cursor.head..], b"ef");
}

#[test]
fn invalid_prefixes_do_not_invoke_the_commit_operation() {
    let mut cursor = Cursor {
        bytes: b"abc".to_vec(),
        head: 0,
    };
    let error = match ValidatedPrefix::try_new(&mut cursor, 4, commit) {
        Ok(_) => panic!("oversized prefix was accepted"),
        Err(error) => error,
    };
    assert_eq!(error.attempted(), 4);
    assert_eq!(error.capacity(), 3);
    assert_eq!(cursor.head, 0);
}

#[test]
fn bounded_and_complete_prefixes_need_no_fallible_commit() {
    let mut cursor = Cursor {
        bytes: b"abcdef".to_vec(),
        head: 0,
    };
    ValidatedPrefix::up_to(&mut cursor, 3, commit).commit();
    ValidatedPrefix::all(&mut cursor, commit).commit();
    assert_eq!(cursor.head, 6);
}

#[test]
fn native_buffer_prefix_commit_has_no_second_validation_surface() {
    let mut buffer = RollingBuffer::<16>::new();
    buffer.try_extend_from_slice(b"abcdef").unwrap();
    buffer.try_consume_prefix(4).unwrap().commit();
    assert_eq!(buffer.as_slice(), b"ef");
    assert_eq!(buffer.consume_prefix_up_to(8), 2);
    assert!(buffer.is_empty());
}

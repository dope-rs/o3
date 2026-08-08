use std::num::NonZeroUsize;

use o3::buffer::{
    PrefixConsumer,
    queue::Ring,
    storage::Owned,
    write::{ByteSink, SliceWriter},
};

fn write_pair<W: ByteSink>(out: &mut W, first: &[u8], second: &[u8]) -> Result<(), W::Error> {
    out.write_slices([first, second])
}

fn write_one<W: ByteSink>(out: &mut W, byte: u8) -> Result<(), W::Error> {
    out.write_byte(byte)
}

#[test]
fn one_generic_sink_covers_growing_exact_and_slice_outputs() {
    let mut growing = Vec::new();
    write_pair(&mut growing, b"one", b"two").unwrap();
    assert_eq!(growing, b"onetwo");

    let mut owned = Owned::try_with_capacity(6).unwrap();
    {
        let mut writer = owned.spare_writer();
        write_pair(&mut writer, b"one", b"two").unwrap();
    }
    assert_eq!(owned.as_slice(), b"onetwo");

    let mut bytes = [0; 6];
    let mut writer = SliceWriter::new(&mut bytes);
    write_pair(&mut writer, b"one", b"two").unwrap();
    assert_eq!(writer.finish(), 6);
    assert_eq!(&bytes, b"onetwo");
}

#[test]
fn one_generic_sink_writes_bytes_without_a_slice_adapter() {
    let mut growing = Vec::new();
    write_one(&mut growing, b'a').unwrap();
    assert_eq!(growing, b"a");

    let mut owned = Owned::try_with_capacity(1).unwrap();
    {
        let mut writer = owned.spare_writer();
        write_one(&mut writer, b'b').unwrap();
    }
    assert_eq!(owned.as_slice(), b"b");

    let mut bytes = [0; 1];
    let mut writer = SliceWriter::new(&mut bytes);
    write_one(&mut writer, b'c').unwrap();
    assert_eq!(writer.finish(), 1);
    assert_eq!(bytes, [b'c']);

    let mut ring = Ring::with_capacity(NonZeroUsize::new(2).unwrap());
    write_one(&mut ring, b'd').unwrap();
    write_one(&mut ring, b'e').unwrap();
    ring.try_consume_prefix(1).unwrap().commit();
    write_one(&mut ring, b'f').unwrap();
    assert_eq!(ring.as_slices(), (&b"e"[..], &b"f"[..]));
}

#[test]
fn aggregate_slice_failure_does_not_commit_a_prefix() {
    let mut owned = Owned::try_with_capacity(5).unwrap();
    {
        let mut writer = owned.spare_writer();
        let error = writer
            .write_slices([b"abc".as_slice(), b"def".as_slice()])
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "capacity exceeded: attempted 6, capacity 5"
        );
        assert_eq!(writer.len(), 0);
    }
    assert_eq!(owned.len(), 0);

    let mut bytes = [0; 5];
    let mut writer = SliceWriter::new(&mut bytes);
    assert!(write_pair(&mut writer, b"abc", b"def").is_err());
    assert_eq!(writer.finish(), 0);
    assert_eq!(bytes, [0; 5]);
}

#[test]
fn aggregate_ring_write_wraps_atomically() {
    let mut ring = Ring::with_capacity(NonZeroUsize::new(8).unwrap());
    ring.try_extend(b"abcdef").unwrap();
    ring.try_consume_prefix(5).unwrap().commit();

    write_pair(&mut ring, b"ghi", b"jkl").unwrap();
    assert_eq!(ring.as_slices(), (&b"fgh"[..], &b"ijkl"[..]));

    let before = {
        let (first, second) = ring.as_slices();
        [first, second].concat()
    };
    assert!(write_pair(&mut ring, b"m", b"n").is_err());
    let (first, second) = ring.as_slices();
    assert_eq!([first, second].concat(), before);
}

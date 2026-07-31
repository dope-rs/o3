use o3::buffer::{ByteRing, ByteSink, Owned, SliceWriter};

fn write_pair<W: ByteSink>(out: &mut W, first: &[u8], second: &[u8]) -> Result<(), W::Error> {
    out.write_slices([first, second])
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
fn aggregate_slice_failure_does_not_commit_a_prefix() {
    let mut owned = Owned::try_with_capacity(5).unwrap();
    {
        let mut writer = owned.spare_writer();
        let error = writer
            .write_slices([b"abc".as_slice(), b"def".as_slice()])
            .unwrap_err();
        assert_eq!(error.attempted(), 6);
        assert_eq!(writer.len(), 0);
    }
    assert!(owned.is_empty());

    let mut bytes = [0; 5];
    let mut writer = SliceWriter::new(&mut bytes);
    assert!(write_pair(&mut writer, b"abc", b"def").is_err());
    assert_eq!(writer.len(), 0);
    assert_eq!(bytes, [0; 5]);
}

#[test]
fn aggregate_ring_write_wraps_atomically() {
    let mut ring = ByteRing::try_with_capacity(8).unwrap();
    ring.try_extend_from_slice(b"abcdef").unwrap();
    ring.try_consume(5).unwrap();

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

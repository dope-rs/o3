pub mod common;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

use std::cell::Cell;

use common::{
    alloc::system::{Activity, Tracker},
    buffer::scatter::{Slices, Source},
};
use o3::buffer::{
    pool,
    raw::Scatter as _,
    segments::scatter::Source as _,
    storage::{self, Owned},
    write::{self, Sink as _, SpareExt as _, consume, edit, spans},
};

#[test]
fn keeps_exact_group() {
    let mut owner = (Owned::try_with(7)).expect("the seven-byte staged owner must allocate");
    let mut writer = owner.spare();
    write::Sink::write_slice(&mut writer, b"ab")
        .as_ref()
        .expect("required fixture outcome");
    let mut exact = writer.exact();
    append(&mut exact);
    assert_eq!(exact.as_mut(), b"cdefg");
    exact
        .try_commit()
        .as_ref()
        .expect("required fixture outcome");
    writer.commit();
    assert_eq!(owner.as_slice(), b"abcdefg");
}

#[test]
fn keeps_bounded_group() {
    let mut owner = (Owned::try_with(7)).expect("the seven-byte staged owner must allocate");
    let mut writer = owner.spare();
    write::Sink::write_slice(&mut writer, b"ab")
        .as_ref()
        .expect("required fixture outcome");
    let mut bounded = writer.bounded();
    append(&mut bounded);
    bounded.commit();
    writer.commit();
    assert_eq!(owner.as_slice(), b"abcdefg");
}

#[test]
fn nests_prefix_edits() {
    let mut owner = Owned::try_with(8).expect("nested prefixes and body");
    let mut outer = edit::Edit::try_new(owner.spare(), b"??").expect("outer prefix");
    {
        let mut inner = edit::Edit::try_new(outer.spare(), b"??").expect("inner prefix");
        inner.write_slice(b"tail").expect("nested body");
        *inner.prefix() = *b"IN";
        inner.commit();
    }
    *outer.prefix() = *b"OU";
    outer.commit();
    assert_eq!(owner.as_slice(), b"OUINtail");
}

#[test]
fn keeps_prefixed_edit() {
    let mut owner = Owned::try_with(7).expect("prefix and tail storage");
    let mut writer = owner.spare();
    writer.write_slice(b"ab").expect("staged prefix");
    let Err(error) = edit::Edit::try_new(writer, b"longer") else {
        panic!("prefix shortfall");
    };
    let error = error.into_parts();
    assert_eq!((error.0.get(), error.1), (6, 5));
    assert!(owner.is_empty());
    {
        let mut output = edit::Edit::try_new(owner.spare(), b"??").expect("discarded prefix");
        let mut body = output.bounded();
        body.write_slice(b"lost").expect("discarded tail");
        body.commit();
        *output.prefix() = *b"NO";
    }
    assert!(owner.is_empty());

    let mut writer = owner.spare();
    writer.write_slice(b"ab").expect("staged prefix");
    let mut output = edit::Edit::try_new(writer, b"??").expect("admitted prefix");
    let mut body = output.bounded();
    body.write_slice(b"efg").expect("disjoint tail");
    body.commit();
    assert_eq!(output.len(), 7);
    *output.prefix() = *b"CD";
    output.commit();
    assert_eq!(owner.as_slice(), b"abCDefg");

    let mut empty = Owned::new();
    let mut output = edit::Edit::try_new(empty.spare(), &[]).expect("empty prefix");
    assert!(output.prefix().is_empty());
    assert!(output.is_empty());
    output.bounded().commit();
    output.commit();
    assert!(empty.is_empty());
}

#[test]
fn keeps_spare_group() {
    let mut owner = (Owned::try_with(7)).expect("the seven-byte staged owner must allocate");
    let mut writer = owner.spare();
    assert_eq!(writer.write(b"ab"), 2);
    append(&mut writer);
    assert_eq!(writer.as_mut_slice(), b"abcdefg");
    writer.commit();
    assert_eq!(owner.as_slice(), b"abcdefg");
}

#[test]
fn keeps_edit_group() {
    let mut lease = (pool::Span::try_new(7)).expect("the seven-byte edit fixture must allocate");
    lease
        .try_append(b"ab")
        .expect("required fixture outcome")
        .commit();
    let mut edit = (lease.try_prefix::<2>(0)).expect("the initialized prefix must be editable");
    edit.prefix().copy_from_slice(b"AB");
    append(&mut edit);
    edit.commit();
    assert_eq!(lease.as_slice(), b"ABcdefg");
}

#[test]
fn rollback_keeps_frames() {
    let mut bytes = Vec::with_capacity(64);
    bytes.extend_from_slice(b"header");
    let pointer = bytes.as_ptr();
    let (_, activity) = Tracker::measure(|| {
        let mut payload = write::Append::new(&mut bytes);
        assert!(payload.is_empty());
        payload.write_slice(b"first").expect("first frame");
        {
            let mut frame = payload.append();
            frame.write_byte(0xff).expect("frame type");
            frame.write_slice(&[0x11, 0x22]).expect("frame body");
            frame.try_fill(4, 0).expect("padding");
            assert_eq!(frame.len(), 7);
            frame.rollback();
        }
        assert_eq!(payload.as_ref(), b"first");
        payload.write_slice(b"second").expect("second frame");
        assert_eq!(payload.len(), 11);
    });
    assert_eq!(activity, [0; 4]);
    assert_eq!(bytes, b"headerfirstsecond");
    assert_eq!(bytes.as_ptr(), pointer);
}

#[test]
fn rejects_vector_growth() {
    let mut bytes = b"header".to_vec();
    let pointer = bytes.as_ptr();
    let source = vec![0; bytes.capacity() + 1];
    let error = Tracker::fail_after(0, || bytes.write_slices([b"prefix", &source]));
    assert_eq!(error, Err(o3::alloc::Error::Exhausted));
    let mut append = write::Append::new(&mut bytes);
    assert_eq!(
        append.try_fill(usize::MAX, 0),
        Err(o3::alloc::Error::Overflow)
    );
    assert!(append.is_empty());
    assert_eq!(bytes, b"header");
    assert_eq!(bytes.as_ptr(), pointer);
}

#[test]
fn writes_without_alloc() {
    let mut owner = storage::Owned::try_with(15).expect("the two frames fit their destination");
    let parts: [&[u8]; 3] = [b"cd", b"", b"ef"];
    let source = (Slices::try_new(&parts)).expect("the source length must fit");
    let mut cursor = source.chunks();
    let (result, activity) = Tracker::activity(|| -> Result<(), write::Error> {
        let mut writer = owner.spare();
        let mut first = spans::Tail::<2, 2>::try_new(&mut writer, b"h:", [b"ab"])?;
        let (header, body) = first.split();
        assert_eq!(header, b"h:");
        assert_eq!(body, b"ab");
        body.make_ascii_uppercase();
        first.commit(b"#!");
        let mut second =
            spans::Tail::<2, 2>::try_scatter(&mut writer, b"h:", cursor.take_bytes(4), b"|")?;
        second.split().1.make_ascii_uppercase();
        second.commit(b"#!");
        writer.commit();
        Ok(())
    });
    assert!(activity == Activity::default());
    result.as_ref().expect("required fixture outcome");
    assert_eq!(cursor.remaining(), 0);
    assert_eq!(owner.as_slice(), b"h:AB#!h:CDEF|#!");
}

fn append(out: &mut impl write::Sink<Error = consume::Error>) {
    out.write_slice(b"c")
        .as_ref()
        .expect("required fixture outcome");
    out.write_slices([])
        .as_ref()
        .expect("required fixture outcome");
    let cause = out
        .write_slices([b"de".as_slice(), b"fgh".as_slice()])
        .expect_err("the second slice must exceed the reserved span");
    let (required, room) = cause.into_parts();
    assert_eq!((required.get(), room), (3, 2));
    out.write_byte(b'd')
        .as_ref()
        .expect("required fixture outcome");
    out.write_slices([b"ef".as_slice(), b"g".as_slice()])
        .as_ref()
        .expect("required fixture outcome");
}

#[test]
fn admission_then_init() {
    let calls = Cell::new(Some(0));
    let source = Source(b"body", &calls);
    let mut owner = storage::Owned::try_with(6).expect("admitted");
    let mut spare = owner.spare();
    spare.write_slice(b"hi").expect("admitted");
    {
        let mut short = spare.try_exact(3).expect("admitted");
        let error = short.try_init(&source).expect_err("shortfall").into_parts();
        assert_eq!((error.0.get(), error.1), (4, 3));
        assert_eq!(calls.get(), Some(0));
    }
    let mut exact = spare.exact();
    exact.try_init(&source).expect("admitted");
    exact.try_commit().expect("admitted");
    spare.commit();
    assert_eq!(calls.get(), Some(1));
    assert_eq!(owner.as_slice(), b"hibody");
}

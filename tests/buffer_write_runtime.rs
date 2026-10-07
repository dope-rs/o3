use std::{num::NonZero, slice};

use o3::{
    base::num::bounded,
    buffer::{
        pool,
        storage::{Owned, inline},
        write::{self, Array, Sink, Slice, SpareExt as _, Staged, consume},
    },
};

#[test]
fn admits_before_ready() {
    let mut bytes = b"xxx".to_vec();
    bytes.clear();
    let mut stage = Staged::<u32>::try_new(&mut bytes, 3).expect("stage");
    let mut spare = stage.spare();
    let mut calls = 0;
    let result = spare.try_append(b"long", || {
        calls += 1;
        Ok::<_, consume::Error>(())
    });
    assert!(result.is_err());
    assert_eq!(calls, 0);
    let result = spare.try_append(b"yes", || {
        calls += 1;
        consume::Error::check(1, 0)
    });
    assert!(result.is_err());
    assert_eq!(calls, 1);
    assert_eq!(spare.len(), 0);
    drop(spare);
    drop(stage);
    // SAFETY: clear preserved three initialized bytes in this allocation,
    // and the staged writer neither reallocates nor deinitializes them.
    assert_eq!(unsafe { slice::from_raw_parts(bytes.as_ptr(), 3) }, b"xxx");
    let mut stage = Staged::<u32>::try_new(&mut bytes, 3).expect("stage");
    let mut spare = stage.spare();
    let value = spare
        .try_append(b"yes", || {
            calls += 1;
            Ok::<_, consume::Error>(7)
        })
        .expect("ready");
    assert_eq!(value, 7);
    assert_eq!(calls, 2);
    spare.commit();
    stage.commit();
    assert_eq!(bytes, b"yes");
}

#[test]
fn appends_vector_batches() {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(b"prefi");
    out.write_byte(b'x').expect("the final prefix byte fits");
    let pointer = out.as_ptr();

    out.write_slices([])
        .expect("an empty batch keeps the prefix");
    out.write_slices([b"xy".as_slice(), &[], b"xy".as_slice()])
        .expect("the existing allocation admits both source occurrences");
    assert_eq!(out.as_ptr(), pointer);
    assert_eq!(out, b"prefixxyxy");

    out.write_slices([b"1234".as_slice(), b"5678".as_slice()])
        .expect("the vector grows for the complete batch");
    assert_eq!(out, b"prefixxyxy12345678");
}

#[test]
fn rejects_pair_atomically() {
    let mut small = inline::Bytes::<6>::new();
    assert!(
        small
            .write_slices([b"abcd".as_slice(), b"efg".as_slice()])
            .is_err()
    );
    assert!(small.as_ref().is_empty());
    small
        .write_slices([b"one", b"two"])
        .expect("the exact batch fits after rejection");
    assert_eq!(small.as_ref(), b"onetwo");

    let pool =
        (pool::Pool::try_new(1, 6)).expect("the single-slot six-byte pooled sink must allocate");
    let mut pooled = (pool.cursor()).expect("the fresh pool has one available buffer");
    assert!(
        pooled
            .write_slices([b"abcd".as_slice(), b"efg".as_slice()])
            .is_err()
    );
    assert!(pooled.as_ref().is_empty());
    pooled
        .write_slices([b"one", b"two"])
        .expect("the exact batch fits after rejection");
    assert_eq!(pooled.as_ref(), b"onetwo");
}

#[test]
fn replaces_inline_suffix() {
    let mut bytes = inline::Bytes::<8>::from_slice(b"original").expect("source bytes");
    let error = bytes
        .try_replace(2, 5, [b"ab".as_slice(), b"cd".as_slice()])
        .expect_err("logical ceiling rejects the complete replacement");
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (2, 1));
    assert_eq!(bytes.as_ref(), b"original");
    bytes
        .try_replace(2, 5, [b"x".as_slice(), b"yz".as_slice()])
        .expect("exact replacement");
    assert_eq!(bytes.as_ref(), b"orxyz");
    bytes
        .try_replace(usize::MAX, usize::MAX, [b"end".as_slice()])
        .expect("clamped append");
    assert_eq!(bytes.as_ref(), b"orxyzend");
}

#[test]
fn inline_preserves_suffix() {
    let mut wide = inline::Bytes::<260, u16>::new();
    wide.backing_mut().fill(0xa5);
    wide.write_slice(&[b'a'; 255])
        .expect("the initialized prefix fits");

    assert!(matches!(
        wide.write_slices([&b"abc"[..], &b"def"[..]]),
        Err(error) if {
            let (required, available) = error.into_parts();
            (required.get(), available) == (3, 2)
        }
    ));
    assert_eq!(wide.len(), 255);
    assert_eq!(&wide.backing_mut()[255..], &[0xa5; 5]);

    wide.write_slices([]).expect("an empty batch fits");
    assert_eq!(wide.len(), 255);
    wide.write_slices([&b"ab"[..], &[], &b"cde"[..]])
        .expect("the exact suffix fits across the u8 length boundary");
    assert_eq!(wide.len(), 260);
    assert_eq!(&wide.as_ref()[..255], &[b'a'; 255]);
    assert_eq!(&wide.as_ref()[255..], b"abcde");

    wide.truncate(255);
    wide.as_mut().fill(b'z');
    assert_eq!(wide.as_ref(), &[b'z'; 255]);
    assert_eq!(&wide.backing_mut()[255..], b"abcde");
}

#[test]
fn keeps_rejected_sources() {
    let mut bytes = [0xa5; 8];
    let mut writer = Slice::new(&mut bytes);
    writer
        .write_byte(b'a')
        .as_ref()
        .expect("required fixture outcome");
    writer
        .write_slice(b"bc")
        .as_ref()
        .expect("required fixture outcome");
    writer
        .write_slices([])
        .expect("an empty batch keeps the prefix");
    let error = writer
        .write_slices([&b"defg"[..], &b"hi"[..]])
        .expect_err("the combined sources must be rejected before either is copied");
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(1), 1)
    );
    let written = writer.into_mut();
    assert_eq!(written, b"abc");
    written[0] = b'A';
    assert_eq!(&bytes, b"Abc\xa5\xa5\xa5\xa5\xa5");
    let mut writer = Slice::new(&mut bytes[3..]);
    writer
        .write_slices([&b"de"[..], &b"fgh"[..]])
        .as_ref()
        .expect("required fixture outcome");
    let error = writer
        .write_byte(b'c')
        .expect_err("a full slice must reject another byte");
    assert_eq!(error.into_parts(), (NonZero::<usize>::MIN, 0));
    writer
        .write_slices([&[]])
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(writer.finish(), 5);
    assert_eq!(&bytes, b"Abcdefgh");
}

#[test]
fn array_rejects_atomically() {
    let mut bytes = [0xa5; 8];
    let mut writer = Array::new(&mut bytes);
    writer
        .write_slice(b"abc")
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(
        writer.write_slices([&b"defg"[..], &b"hi"[..]]),
        Err(bounded::Error {
            value: 7,
            incoming: NonZero::<usize>::MIN.saturating_add(1),
            maximum: 8,
        }),
    );
    assert_eq!(writer.finish().get(), 3);
    assert_eq!(&bytes, b"abc\xa5\xa5\xa5\xa5\xa5");
}

#[test]
fn array_retry_keeps_bound() {
    let mut bytes = [0; 8];
    let mut writer = Array::new(&mut bytes);
    writer
        .write_byte(b'a')
        .as_ref()
        .expect("required fixture outcome");
    writer
        .write_slice(b"bc")
        .as_ref()
        .expect("required fixture outcome");
    assert!(writer.write_slices([&b"defg"[..], &b"hi"[..]]).is_err());
    writer
        .write_slices([&b"de"[..], &b"fgh"[..]])
        .as_ref()
        .expect("required fixture outcome");
    assert!(writer.write_byte(b'i').is_err());
    writer
        .write_slices([&[], &[]])
        .as_ref()
        .expect("required fixture outcome");
    let count: bounded::U32<0, 16> = writer.finish().into_u32();
    assert_eq!(count.get(), 8);
    assert_eq!(&bytes, b"abcdefgh");
}

#[test]
fn empty_array_accepts() {
    let mut bytes = [];
    let mut writer = Array::new(&mut bytes);
    writer
        .write_slices([&[], &[]])
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(
        writer.write_byte(1),
        Err(bounded::Error {
            value: 0,
            incoming: NonZero::<usize>::MIN,
            maximum: 0,
        }),
    );
    assert_eq!(writer.finish().get(), 0);
}

#[test]
fn checks_slice_completion() {
    let mut bytes = [0; 4];
    let mut writer = write::Slice::new(&mut bytes);
    write::Sink::write_slice(&mut writer, b"ab").expect("prefix must fit");
    let error = writer.try_complete().expect_err("suffix is unwritten");
    let (expected, actual) = error.into_parts();
    assert_eq!((expected.get(), actual), (4, 2));
    let mut writer = write::Slice::new(&mut bytes);
    write::Sink::write_slice(&mut writer, b"abcd").expect("exact output must fit");
    writer.try_complete().expect("complete slice must succeed");
    write::Slice::new(&mut [])
        .try_complete()
        .expect("empty slice is complete");
}

#[test]
fn keeps_usize_length() {
    let error = consume::Error::check(usize::MAX, usize::MAX - 1).expect_err("one byte is missing");
    let (expected, actual) = error.into_parts();
    assert_eq!((expected.get(), actual), (usize::MAX, usize::MAX - 1));
}

#[test]
fn writes_fitting_prefix() {
    let mut owner = (Owned::try_with(3)).expect("the three-byte spare owner must allocate");
    let mut writer = owner.spare();
    assert_eq!(writer.write(b"abcdef"), 3);
    assert_eq!(writer.write(b"x"), 0);
    assert_eq!(writer.write(b""), 0);
    assert_eq!(writer.as_mut_slice(), b"abc");
    writer.commit();
    assert_eq!(owner.as_slice(), b"abc");
}

#[test]
fn drains_original_tail() {
    let mut owner = (Owned::try_with(5)).expect("the staged destination must allocate");
    let bytes = b"abcdef";
    let mut source = bytes.as_slice();
    let mut writer = owner.spare();
    assert_eq!(writer.write(b"p"), 1);
    writer.drain(&mut source);
    assert_eq!(owner.as_slice(), b"pabcd");
    assert_eq!(source, b"ef");
    assert_eq!(source.as_ptr(), bytes[4..].as_ptr());
    owner.spare().drain(&mut source);
    assert_eq!(source, b"ef");
    assert_eq!(owner.as_slice(), b"pabcd");
    let mut empty = Owned::new();
    empty.spare().drain(&mut source);
    assert_eq!(source, b"ef");
    assert!(empty.is_empty());
}

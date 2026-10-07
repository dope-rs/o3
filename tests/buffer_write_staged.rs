use std::num::NonZero;

use o3::{
    alloc::VecExt as _,
    buffer::{
        raw::Scatter as _,
        storage::Owned,
        write::{self, Reserved, Sink as _, SpareExt as _, Staged, spans},
    },
};

#[test]
fn rejects_reservation() {
    let mut bytes = b"old".to_vec();
    assert!(Staged::<usize>::try_reserve(&mut bytes, usize::MAX).is_err());
    assert!(matches!(
        Staged::<usize>::try_grow(&mut bytes, usize::MAX),
        Err(o3::alloc::Error::Overflow)
    ));
    assert_eq!(bytes, b"old");
}

#[test]
fn stages_reserved_suffix() {
    let mut bytes = Vec::try_new(8).expect("reserved backing");
    bytes.extend_from_slice(b"old");
    let pointer = bytes.as_ptr();
    {
        let _space = Reserved::try_exact(&mut bytes, 5).expect("retained suffix");
    }
    assert_eq!(bytes, b"old");
    let reserved = Reserved::try_exact(&mut bytes, 5).expect("reused suffix");
    let mut staged = reserved.into_staged();
    staged.write_slice(b"next!").expect("complete suffix");
    assert_eq!(
        staged.try_complete().expect("exact completion").commit(),
        b"next!"
    );
    assert_eq!(bytes, b"oldnext!");
    assert_eq!(bytes.as_ptr(), pointer);
    assert!(Reserved::try_exact(&mut bytes, usize::MAX).is_err());
    assert_eq!(bytes, b"oldnext!");
}

#[test]
fn stages_available_suffix() {
    let mut bytes = Vec::try_new(8).expect("reserved backing");
    bytes.extend_from_slice(b"old");
    let pointer = bytes.as_ptr();
    {
        let _space = Reserved::from(&mut bytes);
    }
    {
        let mut staged = Reserved::from(&mut bytes).into_staged();
        staged.write_slice(b"lost").expect("unpublished bytes");
    }
    assert_eq!(bytes, b"old");
    let mut staged = Reserved::from(&mut bytes).into_staged();
    assert_eq!(staged.capacity(), 5);
    let error = staged.write_slice(b"beyond").expect_err("suffix limit");
    assert_eq!(
        error.into_parts(),
        (NonZero::<usize>::MIN.saturating_add(5), 5)
    );
    assert!(staged.is_empty());
    staged.write_slice(b"next!").expect("complete suffix");
    assert_eq!(
        staged.try_complete().expect("exact completion").commit(),
        b"next!"
    );
    assert_eq!(bytes, b"oldnext!");
    assert_eq!(bytes.as_ptr(), pointer);
    assert!(
        Reserved::from(&mut bytes)
            .into_staged()
            .try_complete()
            .expect("empty suffix")
            .commit()
            .is_empty()
    );
}

#[test]
fn grows_exactly() {
    let mut bytes = b"old".to_vec();
    let mut staged = Staged::<u32>::try_grow(&mut bytes, 2).expect("exact suffix");
    staged.write_slice(b"xy").expect("admitted bytes");
    staged.commit();
    assert_eq!(bytes, b"oldxy");
    assert_eq!(bytes.capacity(), 5);
}

#[test]
fn retains_backing() {
    let mut bytes = Vec::try_new(16).expect("reserved backing");
    bytes.extend_from_slice(b"prefix");
    let pointer = bytes.as_ptr();
    let capacity = bytes.capacity();
    let suffix = {
        let mut staged = Staged::<u32>::try_new(&mut bytes, 5).expect("reserved suffix");
        assert!(staged.is_empty());
        assert_eq!(staged.capacity(), 5);
        staged.write_byte(b'a').expect("first byte");
        let mut spare = staged.spare();
        spare.write_slice(b"b").expect("child fragment");
        spare.commit();
        staged
            .write_slices([b"c".as_slice(), b"".as_slice(), b"de".as_slice()])
            .expect("remaining fragments");
        assert_eq!(staged.len(), 5);
        assert!(matches!(staged.write_slice(b"f"), Err(error)
            if error.into_parts() == (NonZero::<usize>::MIN, 0)));
        staged.commit()
    };
    assert_eq!(suffix, b"abcde");
    assert_eq!(suffix.as_ptr(), pointer.wrapping_add(6));
    suffix[0] = b'A';
    assert_eq!(bytes, b"prefixAbcde");
    assert_eq!(bytes.as_ptr(), pointer);
    assert_eq!(bytes.capacity(), capacity);
}

#[test]
fn retains_capacity() {
    let mut bytes = Vec::try_new(7).expect("reserved backing");
    bytes.extend_from_slice(b"abc");
    assert!(matches!(Staged::<u32>::try_new(&mut bytes, 5), Err(error)
        if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(4), 4)));
    assert_eq!(bytes, b"abc");
    let mut staged = Staged::<u32>::try_new(&mut bytes, 3).expect("logical limit");
    staged.write_slice(b"d").expect("first byte");
    assert!(
        matches!(staged.write_slices([b"x".as_slice(), b"yz".as_slice()]), Err(error)
        if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(1), 1))
    );
    assert_eq!(staged.len(), 1);
    staged.write_slice(b"ef").expect("remaining bytes");
    staged.commit();
    assert_eq!(bytes, b"abcdef");
}

#[test]
fn empty_stage() {
    let mut bytes = Vec::new();
    let pointer = bytes.as_ptr();
    let mut staged = Staged::<u32>::try_new(&mut bytes, 0).expect("empty stage");
    staged.write_slice(b"").expect("empty write");
    staged.write_slices([]).expect("empty batch");
    staged
        .write_slices([b"".as_slice(); 3])
        .expect("empty fragments");
    staged
        .spare()
        .try_complete()
        .expect("empty suffix complete");
    let suffix = staged
        .try_complete()
        .expect("empty stage complete")
        .commit();
    assert!(suffix.is_empty());
    assert_eq!(suffix.as_ptr(), pointer);
    assert!(bytes.is_empty());
    assert_eq!(bytes.capacity(), 0);
    assert!(
        matches!(Staged::<u32>::try_new(&mut bytes, u32::MAX), Err(error)
        if error.into_parts() == (NonZero::new(u32::MAX as usize).expect("positive limit"), 0))
    );
}

#[test]
fn unpublished_stages() {
    let mut bytes = Vec::try_new(8).expect("reserved backing");
    bytes.extend_from_slice(b"old");
    {
        let mut staged = Staged::<u32>::try_new(&mut bytes, 5).expect("reserved suffix");
        staged.write_slice(b"lost").expect("staged bytes");
    }
    assert_eq!(bytes, b"old");
    let mut staged = Staged::<u32>::try_new(&mut bytes, 5).expect("reused suffix");
    staged.write_slice(b"new").expect("replacement bytes");
    assert_eq!(staged.commit(), b"new");
    assert_eq!(bytes, b"oldnew");
}

#[test]
fn discards_spare() {
    let mut bytes = Vec::try_new(8).expect("reserved backing");
    let mut staged = Staged::<u32>::try_new(&mut bytes, 8).expect("reserved suffix");
    staged.write_slice(b"ab").expect("first fragment");
    {
        let mut spare = staged.spare();
        spare.write_slice(b"lost").expect("discarded fragment");
    }
    assert_eq!(staged.len(), 2);
    let mut spare = staged.spare();
    spare.write_slice(b"gap").expect("incomplete suffix");
    assert!(spare.try_complete().is_err());
    assert_eq!(staged.len(), 2);
    let mut spare = staged.spare();
    spare.fill(b'x');
    spare.try_complete().expect("full suffix");
    staged.commit();
    assert_eq!(bytes, b"abxxxxxx");
}

#[test]
fn stages_native_extent() {
    let mut bytes = Vec::try_new(8).expect("reserved backing");
    bytes.extend_from_slice(b"old");
    let error =
        Staged::<usize>::try_new(&mut bytes, usize::MAX).expect_err("native limit exceeds backing");
    assert_eq!(error.into_parts(), (NonZero::<usize>::MAX, 5));
    {
        let mut staged = Staged::<usize>::try_new(&mut bytes, 5).expect("native suffix");
        staged.write_slice(b"lost").expect("unpublished bytes");
    }
    assert_eq!(bytes, b"old");
    let mut staged = Staged::<usize>::try_new(&mut bytes, 5).expect("reused suffix");
    assert!(
        staged
            .write_slices([b"abc".as_slice(), b"def".as_slice()])
            .is_err()
    );
    assert!(staged.is_empty());
    staged
        .write_slices([b"ne".as_slice(), b"w".as_slice()])
        .expect("native write");
    staged.commit();
    assert_eq!(bytes, b"oldnew");
}

#[test]
fn rejects_short_stage() {
    let mut bytes = b"old".to_vec();
    let mut staged = Staged::<usize>::try_reserve(&mut bytes, 3).expect("suffix");
    staged.write_slice(b"ab").expect("partial suffix");
    assert!(matches!(staged.try_complete(), Err(error)
        if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(2), 2)));
    assert_eq!(bytes, b"old");
}

#[test]
fn rejects_oversized_stage() {
    let mut bytes = b"old".to_vec();
    let mut staged = Staged::<usize>::try_reserve(&mut bytes, 3).expect("suffix");
    staged.write_byte(b'a').expect("first byte");
    assert!(matches!(staged.write_slice(b"bcd"), Err(error)
        if error.into_parts() == (NonZero::<usize>::MIN.saturating_add(2), 2)));
    staged.write_slice(b"").expect("empty write");
    staged.write_slice(b"bc").expect("remaining suffix");
    staged.try_complete().expect("complete emission").commit();
    assert_eq!(bytes, b"oldabc");
}

#[test]
fn edits_appended_suffix() {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(b"prefix");
    let pointer = bytes.as_ptr();
    let mut append = write::Append::new(&mut bytes);
    append.try_grow(8).expect("reserved suffix");
    append.write_slice(b"tail").expect("suffix");
    {
        let mut nested = append.append();
        nested.write_slice(b"discard").expect("nested suffix");
        nested.as_mut().fill(0);
        nested.rollback();
    }
    assert_eq!(append.start(), 6);
    assert_eq!(append.as_ref(), b"tail");
    let tail = append.into_mut();
    tail.copy_from_slice(b"TAIL");
    assert_eq!(bytes, b"prefixTAIL");
    assert_eq!(bytes.as_ptr(), pointer);
    let mut append = write::Append::new(&mut bytes);
    append.write_slice(b"kept").expect("suffix");
    let mut suffix = append.into_append();
    suffix.write_slice(b"discard").expect("nested suffix");
    suffix.rollback();
    assert_eq!(bytes, b"prefixTAILkept");
    assert_eq!(bytes.as_ptr(), pointer);
}

#[test]
fn commits_reserved_tail() {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(b"prefix");
    let pointer = bytes.as_ptr();
    let mut tail = write::Append::new(&mut bytes)
        .try_tail::<2, 2>([b"bo", b"dy"], 2, b'.')
        .expect("reserved tail");
    assert_eq!(tail.as_ref(), b"body..");
    tail.as_mut()[0] = b'B';
    let bytes = tail.commit(b"ok").into_mut();
    assert_eq!(bytes, b"Body..ok");
    assert_eq!(bytes.as_ptr(), pointer.wrapping_add(6));
}

#[test]
fn rejects_tail_overflow() {
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(b"prefix");
    let pointer = bytes.as_ptr();
    let mut append = write::Append::new(&mut bytes);
    append.write_slice(b"body").expect("body");
    let rejected = append
        .try_tail::<2, 0>([], usize::MAX, 0)
        .expect_err("overflow");
    let (append, error) = rejected.into_parts();
    assert_eq!(error, o3::alloc::Error::Overflow);
    assert_eq!(append.start(), 6);
    assert_eq!(append.as_ref(), b"body");
    append.rollback();
    assert_eq!(bytes, b"prefix");
    assert_eq!(bytes.as_ptr(), pointer);
}

#[test]
fn discards_unwritten_tail() {
    let mut bytes = b"prefix".to_vec();
    {
        let tail = write::Append::new(&mut bytes)
            .try_tail::<16, 1>([b"body"], 0, 0)
            .expect("reserved tail");
        assert_eq!(tail.as_ref(), b"body");
    }
    assert_eq!(bytes, b"prefixbody");
    write::Append::new(&mut bytes)
        .try_tail::<16, 1>([b"discard"], 0, 0)
        .expect("reserved tail")
        .rollback();
    assert_eq!(bytes, b"prefixbody");
}

#[test]
fn rollback_after_growth() {
    let mut bytes = b"header".to_vec();
    let capacity = bytes.capacity();
    let mut payload = write::Append::new(&mut bytes);
    payload.try_fill(1, 0x55).expect("padding");
    assert_eq!(payload.len(), 1);
    payload.rollback();
    assert_eq!(bytes, b"header");
    assert!(bytes.capacity() >= 2 * capacity);
}

#[test]
fn commits_empty_span() {
    let mut owner = Owned::new();
    let mut writer = owner.spare();
    let Err(cause) = writer.try_exact(usize::MAX) else {
        panic!("the largest span must exceed the empty owner");
    };
    let (required, room) = cause.into_parts();
    assert_eq!((required.get(), room), (usize::MAX, 0));
    let mut exact = writer.exact();
    let cause =
        write::Sink::write_byte(&mut exact, 1).expect_err("an empty span must reject a byte");
    let (required, room) = cause.into_parts();
    assert_eq!((required.get(), room), (1, 0));
    write::Sink::write_slice(&mut exact, b"")
        .as_ref()
        .expect("required fixture outcome");
    exact
        .try_commit()
        .as_ref()
        .expect("required fixture outcome");
    writer
        .exact()
        .try_commit()
        .as_ref()
        .expect("required fixture outcome");
    writer.commit();
    assert!(owner.is_empty());
}

#[test]
fn rejects_short_commit() {
    let mut owner = (Owned::try_with(4)).expect("the staged destination must allocate");
    let mut writer = owner.spare();
    assert_eq!(writer.write(b"ab"), 2);
    let error = writer
        .try_complete()
        .expect_err("the missing suffix must reject publication");
    let (expected, actual) = error.into_parts();
    assert_eq!((expected.get(), actual), (4, 2));
    assert!(owner.is_empty());
    let mut writer = owner.spare();
    assert_eq!(writer.write(b"abcd"), 4);
    writer
        .try_complete()
        .as_ref()
        .expect("required fixture outcome");
    assert_eq!(owner.as_slice(), b"abcd");
    Owned::new()
        .spare()
        .try_complete()
        .as_ref()
        .expect("required fixture outcome");
}

#[test]
fn cancels_publication() {
    let mut owner = (Owned::try_with(9)).expect("the staged owner must allocate");
    let mut writer = owner.spare();
    assert_eq!(writer.write(b"ok"), 2);
    {
        let mut frame = (spans::Tail::<0, 3>::try_new(&mut writer, &[], [b"body"]))
            .expect("the unpublished frame must fit");
        frame.split().1.fill(0xff);
    }
    assert_eq!(writer.as_mut_slice(), b"ok");
    assert_eq!(writer.remaining(), 7);
    let frame = (spans::Tail::<0, 3>::try_new(&mut writer, &[], [b"body"]))
        .expect("cancellation must preserve the complete reservation");
    frame.commit(b"end");
    writer.commit();
    assert_eq!(owner.as_slice(), b"okbodyend");
}

#[test]
fn rejects_before_traversal() {
    let mut source = &b"body"[..];
    let mut owner = Owned::new();
    let mut writer = owner.spare();
    let Err(write::Error::Capacity(error)) =
        spans::Tail::<2, 3>::try_scatter(&mut writer, b"hi", source.take_bytes(4), b"!")
    else {
        panic!("the complete frame must be rejected");
    };
    let (required, room) = error.into_parts();
    assert_eq!((required.get(), room), (10, 0));
    assert_eq!(source.remaining(), 4);
    assert_eq!(writer.len(), 0);
}

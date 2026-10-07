use std::mem;

use o3::buffer::{pool, resident, segments, storage, write::SpareExt as _};

#[test]
fn leases_keep_their_pool() {
    let first = pool::Pool::try_new(1, 4).expect("first pool");
    let second = pool::Pool::try_new(1, 8).expect("second pool");
    {
        let mut right = second.acquire().expect("second slot");
        {
            let mut left = first.acquire().expect("first slot");
            left.try_append(b"left").expect("left bytes fit").commit();
            right
                .try_append(b"right")
                .expect("right bytes fit")
                .commit();
            mem::swap(&mut *left, &mut *right);
            assert_eq!((left.as_slice(), left.capacity().get()), (&b"right"[..], 8));
            assert_eq!(
                (right.as_slice(), right.capacity().get()),
                (&b"left"[..], 4)
            );
        }
        assert_eq!((first.available(), second.available()), (0, 1));
    }
    assert_eq!((first.available(), second.available()), (1, 1));
}

#[test]
fn keeps_initialized_bytes() {
    let mut left = storage::Owned::try_filled(3, b'a').expect("first owner");
    let mut right = storage::Owned::try_with(8).expect("second owner");
    right.try_extend(b"bc").expect("initialized prefix");
    mem::swap(&mut *left, &mut *right);
    assert_eq!((left.as_slice(), left.capacity()), (&b"bc"[..], 8));
    assert_eq!((right.as_slice(), right.capacity()), (&b"aaa"[..], 3));
    let mut spare = left.spare();
    assert_eq!(spare.write(b"def"), 3);
    spare.commit();
    assert_eq!(left.freeze().as_slice(), b"bcdef");
    assert_eq!(right.freeze().as_slice(), b"aaa");
}

#[test]
fn keeps_retained_backing() {
    let mut left = storage::Shared::try_copy(b"abc").expect("first region");
    let retained = {
        let mut right = storage::Shared::try_copy(b"defgh").expect("second region");
        let retained = left.try_clone().expect("retained region");
        let first = left.as_slice().as_ptr();
        let second = right.as_slice().as_ptr();
        mem::swap(&mut *left, &mut *right);
        assert_eq!(left.as_slice().as_ptr(), second);
        assert_eq!(right.as_slice().as_ptr(), first);
        retained
    };
    assert_eq!(retained.as_slice(), b"abc");
    assert_eq!(left.as_slice(), b"defgh");
}

#[test]
fn keeps_metadata_store() {
    let first = segments::Pool::try_new(segments::Plan {
        segments: 1,
        chains: 0,
    })
    .expect("single pool");
    let second = segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    })
    .expect("multi pool");
    let mut left = first.try_chain().expect("first chain");
    {
        let mut right = second.try_chain().expect("second chain");
        left.try_push(resident::Shared::from(&b"a"[..]))
            .expect("single node");
        right
            .try_push(resident::Shared::from(&b"bc"[..]))
            .expect("first node");
        right
            .try_push(resident::Shared::from(&b"def"[..]))
            .expect("second node");
        mem::swap(&mut *left, &mut *right);
        assert_eq!((left.len(), right.len()), (5, 1));
        left.clear();
        assert!(second.is_idle());
        assert!(!first.is_idle());
    }
    assert!(first.is_idle());
}

pub mod common;

use std::io::Read as _;

use common::{buffer::Bytes as _, depot::model::Lcg};
use o3::buffer::{
    segments::{self, scatter},
    storage,
};

#[test]
fn split_keeps_owners() {
    let first = (storage::Shared::try_copy(b"abc")).expect("the prefix owner must allocate");
    let second = (storage::Shared::try_copy(b"def")).expect("the tail owner must allocate");
    let ptrs = [first.as_slice().as_ptr(), second.as_slice().as_ptr()];

    let (mut chain, prefix) = {
        let pool = (segments::Pool::try_new(segments::Plan {
            segments: 2,
            chains: 1,
        }))
        .expect("the split pool must allocate two segments and one chain");
        let mut chain = (pool.try_chain()).expect("metadata edge must fit");
        chain
            .try_push(first)
            .as_ref()
            .expect("metadata admits the owner");
        chain
            .try_push(second)
            .as_ref()
            .expect("metadata admits the owner");
        let prefix = (chain.try_take(3)).expect("segment transition accepted");
        (chain, prefix)
    };

    assert_eq!(prefix.contiguous(), Some(&b"abc"[..]));
    assert_eq!(chain.contiguous(), Some(&b"def"[..]));
    let head = (prefix.segments().next()).expect("prefix has one segment");
    assert_eq!(head.as_slice().as_ptr(), ptrs[0]);
    let tail = (chain.front_entry()).expect("tail has one segment");
    assert_eq!(tail.get().as_slice().as_ptr(), ptrs[1]);
    let tail = tail.pop();
    assert_eq!(tail.as_slice(), b"def");
    assert!(chain.front_entry().is_none());
}

#[test]
fn projects_boundary_owner() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    }))
    .expect("the projection pool must allocate");
    let mut source = (pool.try_chain()).expect("metadata edge must fit");
    let mut target = (pool.try_chain()).expect("metadata edge must fit");
    let first =
        (storage::Shared::try_copy(b"abc")).expect("the projection head owner must allocate");
    let second =
        (storage::Shared::try_copy(b"def")).expect("the projection tail owner must allocate");
    source
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");
    source
        .try_push(second)
        .as_ref()
        .expect("metadata admits the owner");

    source
        .try_move_to(&mut target, 4)
        .as_ref()
        .expect("required fixture outcome");

    assert_eq!(
        target.try_bytes().expect("chain bytes must allocate"),
        b"abcd"
    );
    assert_eq!(
        source.try_bytes().expect("chain bytes must allocate"),
        b"ef"
    );
}

#[test]
fn classifies_without_copy() {
    let empty = (segments::Pool::<storage::Shared>::try_new(segments::Plan {
        segments: 0,
        chains: 0,
    }))
    .expect("the empty classification pool must allocate");
    let empty = (empty.try_chain()).expect("metadata edge must fit");
    assert!(empty.is_empty());

    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    }))
    .expect("the populated classification pool must allocate");
    let owner =
        (storage::Shared::try_copy(b"one")).expect("the single classified owner must allocate");
    let ptr = owner.as_slice().as_ptr();
    let mut single = (pool.try_chain()).expect("metadata edge must fit");
    single
        .try_push(owner)
        .as_ref()
        .expect("metadata admits the owner");
    let owner = match single.into_shape() {
        segments::Shape::One(owner) => Some(owner),
        segments::Shape::Empty | segments::Shape::Many(_) => None,
    }
    .expect("single classification retains the nonempty item");
    assert!(owner.count() == segments::Count::ONE);
    assert_eq!(owner.as_slice().as_ptr(), ptr);

    let mut multi = (pool.try_chain()).expect("metadata edge must fit");
    let first =
        (storage::Shared::try_copy(b"a")).expect("the first multi-segment owner must allocate");
    let second =
        (storage::Shared::try_copy(b"b")).expect("the second multi-segment owner must allocate");
    multi
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");
    multi
        .try_push(second)
        .as_ref()
        .expect("metadata admits the owner");
    assert!(multi.count() == segments::Count::TWO);
    let third = (storage::Shared::try_copy(b"ccc")).expect("the third owner must allocate");
    multi
        .try_push(third)
        .as_ref()
        .expect("metadata admits the owner");
    assert!(multi.count() == segments::Count::new(3));
    {
        let front = (multi.front_entry()).expect("borrowing the front must retain every owner");
        assert_eq!(front.get().as_slice(), b"a");
    }
    let mut left = 5;
    for (i, bytes) in [b"a".as_slice(), b"b", b"ccc"].into_iter().enumerate() {
        let front = (multi.front_entry()).expect("each retained front must remain available once");
        let ptr = front.get().as_slice().as_ptr();
        let owner = front.pop();
        assert_eq!(owner.as_slice().as_ptr(), ptr);
        assert_eq!(owner.as_slice(), bytes);
        left -= bytes.len();
        assert_eq!(multi.len(), left);
        assert!(multi.count() == segments::Count::new(2 - i as u32));
    }
    assert!(multi.front_entry().is_none());
    let reused =
        (storage::Shared::try_copy(b"reused")).expect("the replacement owner must allocate");
    multi
        .try_push(reused)
        .as_ref()
        .expect("metadata admits the owner");
    let front = (multi.front_entry()).expect("the consumed metadata slot must be reusable");
    assert_eq!(front.pop().as_slice(), b"reused");
}

#[test]
fn scoped_is_covariant() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the covariance pool must allocate");
    let scope = ();
    let mut chain = shorten_pool_borrow(pool.scoped(), &scope);
    let owner =
        (storage::Shared::try_copy(b"borrowed")).expect("the covariant scoped owner must allocate");
    chain
        .try_push(owner)
        .as_ref()
        .expect("metadata admits the owner");
    assert_eq!(chain.contiguous(), Some(&b"borrowed"[..]));
    let tail = storage::Shared::try_copy(b"tail").expect("scoped tail owner");
    chain.try_push(tail).as_ref().expect("scoped tail slot");
    assert_eq!(chain.len(), 12);
    assert_eq!(chain.segments().len(), 2);
    assert_eq!(pool.available().segments, 0);
    assert_eq!(pool.available().chains, 0);
    drop(chain);
    assert!(pool.is_idle());
}

#[test]
fn reads_chain() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the scatter-read pool must allocate");
    let mut owned = (pool.try_chain()).expect("metadata edge must fit");
    let first =
        (storage::Shared::try_copy(b"ab")).expect("the detached scatter head must allocate");
    let second =
        (storage::Shared::try_copy(b"c")).expect("the detached scatter tail must allocate");
    owned
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");
    owned
        .try_push(second)
        .as_ref()
        .expect("metadata admits the owner");
    let mut dbuf = Vec::new();
    let result = scatter::Reader::new(owned.segments()).read_to_end(&mut dbuf);
    result.as_ref().expect("detached scatter read succeeds");

    assert_eq!(dbuf, b"abc");
}

#[test]
fn flattens_chain() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the flatten source pool must allocate");
    let mut source = pool.into_chain();
    let first = (storage::Shared::try_copy(b"abc")).expect("the flatten head owner must allocate");
    let second = (storage::Shared::try_copy(b"def")).expect("the flatten tail owner must allocate");
    source
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");
    source
        .try_push(second)
        .as_ref()
        .expect("metadata admits the owner");

    let output = (source.try_flatten()).expect("the six-byte flattened owner must allocate");
    assert!(source.count() == segments::Count::TWO);

    assert_eq!(output.as_slice(), b"abcdef");
    assert_eq!(
        source.try_bytes().expect("chain bytes must allocate"),
        b"abcdef"
    );
}

#[test]
fn rehomes_across_pools() {
    let owner = (storage::Shared::try_copy(b"payload")).expect("the rehomed owner must allocate");
    let ptr = owner.as_slice().as_ptr();
    let spool = (segments::Pool::try_new(segments::Plan {
        segments: 1,
        chains: 0,
    }))
    .expect("the source rehome pool must allocate");
    let tpool = (segments::Pool::try_new(segments::Plan {
        segments: 1,
        chains: 0,
    }))
    .expect("the target rehome pool must allocate");
    let mut source = spool.into_chain();
    let mut target = tpool.into_chain();
    source
        .try_push(owner)
        .as_ref()
        .expect("metadata admits the owner");

    assert!(matches!(
        target.try_append(&mut source),
        Err(segments::Error::DifferentPool)
    ));
    assert!(source.count() == segments::Count::ONE);
    source
        .try_move_to(&mut target, source.len())
        .as_ref()
        .expect("destination admits transferred owners");
    assert!(source.is_empty());
    let owner = (target.segments().next()).expect("rehome publishes one segment");
    assert_eq!(owner.as_slice().as_ptr(), ptr);
}

#[test]
fn matches_flat_model() {
    const VALUES: [&[u8]; 5] = [b"a", b"bc", b"def", b"ghij", b"klmno"];
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 8,
        chains: 4,
    }))
    .expect("the flat-model pool must allocate");
    let mut first = (pool.try_chain()).expect("metadata edge must fit");
    let mut second = (pool.try_chain()).expect("metadata edge must fit");
    let mut want1 = Vec::new();
    let mut want2 = Vec::new();
    let mut rng = Lcg::<1_442_695_040_888_963_407>::new(0x6a09_e667_f3bc_c909u64);

    for _ in 0..2_048 {
        let random = rng.advance();
        match (random >> 32) % 6 {
            0 | 1 => {
                let value = VALUES[(random as usize) % VALUES.len()];
                let target = if (random >> 8) & 1 == 0 {
                    (&mut first, &mut want1)
                } else {
                    (&mut second, &mut want2)
                };
                let owner = (storage::Shared::try_copy(value))
                    .expect("each flat-model owner must allocate");
                if target.0.try_push(owner).is_ok() {
                    target.1.extend_from_slice(value);
                }
            }
            2 => {
                if let Some(value) = first.pop_segment() {
                    assert!(want1.starts_with(value.as_slice()));
                    want1.drain(..value.as_slice().len());
                }
            }
            3 => {
                let amount = (random as usize) % (want1.len() + 2);
                if first.try_move_to(&mut second, amount).is_ok() {
                    want2.extend(want1.drain(..amount));
                }
            }
            4 => {
                if first.try_append(&mut second).is_ok() {
                    want1.append(&mut want2);
                }
            }
            _ => {
                if let Some(value) = second.pop_segment() {
                    assert!(want2.starts_with(value.as_slice()));
                    want2.drain(..value.as_slice().len());
                }
            }
        }

        assert_eq!(first.try_bytes().expect("chain bytes must allocate"), want1);
        assert_eq!(
            second.try_bytes().expect("chain bytes must allocate"),
            want2
        );
        assert_eq!(
            pool.available().segments as u128
                + u128::from(first.count().into_raw())
                + u128::from(second.count().into_raw()),
            pool.capacity().segments as u128
        );
        let multi = usize::from(first.count() >= segments::Count::TWO)
            + usize::from(second.count() >= segments::Count::TWO);
        assert_eq!(pool.available().chains + multi, pool.capacity().chains);
    }
}

fn shorten_pool_borrow<'long: 'short, 'short>(
    chain: segments::Chain<storage::Shared, &'long segments::Header<storage::Shared>>,
    _scope: &'short (),
) -> segments::Chain<storage::Shared, &'short segments::Header<storage::Shared>> {
    chain
}

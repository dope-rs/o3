pub mod common;

use std::{array, io::Read as _, mem, num::NonZero};

use common::{
    alloc::system::Tracker,
    buffer::{try_item, try_many},
};
use o3::{
    buffer::{
        raw::Scatter as _,
        segments::{self, queue, scatter},
        storage,
        write::consume::Consume as _,
    },
    depot::capacity::{self, Count},
};
use scatter::Source as _;

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn insert_amortizes_growth() {
    for front in [false, true] {
        let items: [_; 32] =
            array::from_fn(|index| try_item(&[index as u8]).expect("nonempty owner"));
        let pointers: [_; 32] = array::from_fn(|index| items[index].as_slice().as_ptr());
        let mut items = items.into_iter();
        let first = items.next().expect("first owner");
        let second = items.next().expect("second owner");
        let mut chain = segments::Many::try_from((first, second)).expect("initial pair");
        let (chain, allocs) = Tracker::count(|| {
            for item in items {
                chain = if front {
                    segments::Many::try_from((item, chain))
                        .map_err(|error| error.map(|(item, chain)| (chain, item)))
                } else {
                    segments::Many::try_from((chain, item))
                }
                .expect("growing chain");
            }
            chain
        });
        assert!(
            allocs <= 5,
            "repeated insertion must amortize metadata growth"
        );
        assert_eq!(chain.as_chain().segments().len(), pointers.len());
        for (index, owner) in chain.as_chain().segments().enumerate() {
            let original = if front {
                if index < 30 { 31 - index } else { index - 30 }
            } else {
                index
            };
            assert_eq!(owner.as_slice().as_ptr(), pointers[original]);
        }
    }
}

#[test]
fn joining_amortizes_growth() {
    let chains: [_; 16] = array::from_fn(|index| {
        let first = try_item(&[(index * 2) as u8]).expect("first owner");
        let second = try_item(&[(index * 2 + 1) as u8]).expect("second owner");
        segments::Many::try_from((first, second)).expect("input pair")
    });
    let mut chains = chains.into_iter();
    let mut chain = chains.next().expect("first pair");
    let (chain, allocs) = Tracker::count(|| {
        for source in chains {
            chain = segments::Many::try_from((chain, source)).expect("joined owners");
        }
        chain
    });
    assert!(allocs <= 5, "repeated joins must amortize metadata growth");
    assert!(
        chain
            .as_chain()
            .segments()
            .map(|owner| owner.as_slice()[0])
            .eq(0..32)
    );
}

#[test]
fn growth_uses_local_count() {
    let pool = segments::Pool::try_new(segments::Plan {
        segments: 64,
        chains: 2,
    })
    .expect("shared metadata pool");
    let chain = try_many(&pool).expect("small chain");
    let mut other = pool.try_chain().expect("other chain");
    for _ in 0..62 {
        other
            .try_insert(try_item(b"x").expect("retained owner"))
            .expect("shared node capacity");
    }
    let item = try_item(b"last").expect("incoming owner");
    let bytes = {
        let measured = Tracker::usage(|| {
            segments::Pool::<storage::Shared>::try_new(segments::Plan {
                segments: 4,
                chains: 1,
            })
            .expect("local reservation reference")
        });
        measured.1.1
    };
    let (chain, activity) =
        Tracker::measure(|| segments::Many::try_from((chain, item)).expect("detached small chain"));
    assert_eq!(activity, [1, bytes, 0, 0]);
    assert_eq!(chain.as_chain().segments().len(), 3);
    assert_eq!(other.segments().len(), 62);
}

#[test]
fn chains_share_nodes() {
    const CAPACITY: usize = 4;
    const CHAINS: usize = 2;
    const BYTES: usize = 72 + CAPACITY * 32 + CHAINS * 40;

    let (result, build) = Tracker::measure(|| {
        segments::Pool::<storage::Shared>::try_new(segments::Plan {
            segments: CAPACITY,
            chains: CHAINS,
        })
    });
    let pool = result.expect("the four-segment two-chain pool must allocate");
    assert_eq!(build, [1, BYTES, 0, 0]);

    let result = storage::Shared::try_copy(b"first");
    let first = result.expect("first retained segment must allocate");
    let result = storage::Shared::try_copy(b"second");
    let second = result.expect("second retained segment must allocate");
    let held_a = (first.try_clone()).expect("retained edge must fit");
    let held_b = (second.try_clone()).expect("retained edge must fit");
    let loan_a = (first.try_clone()).expect("retained edge must fit");
    let loan_b = (second.try_clone()).expect("retained edge must fit");
    let ((), hot) = Tracker::measure(|| {
        {
            let mut retained = (pool.try_chain()).expect("retained edge must fit");
            retained
                .try_push(held_a)
                .as_ref()
                .expect("required fixture outcome");
            retained
                .try_push(held_b)
                .as_ref()
                .expect("required fixture outcome");
            let front = retained
                .front_entry()
                .expect("the pooled chain retains its first owner")
                .pop();
            assert_eq!(front.as_slice(), b"first");
            assert_eq!(retained.contiguous(), Some(&b"second"[..]));
            retained
                .try_push(front)
                .as_ref()
                .expect("required fixture outcome");
            let result = retained.try_take(5);
            let mut prefix = result.expect("one retained segment must move into a prefix");
            retained
                .try_append(&mut prefix)
                .as_ref()
                .expect("required fixture outcome");
            let front = (retained.front_entry()).expect("the joined chain retains its first owner");
            let ptr = front.get().as_slice().as_ptr();
            let owner = front.pop();
            assert_eq!(owner.as_slice().as_ptr(), ptr);
            retained
                .try_push(owner)
                .as_ref()
                .expect("required fixture outcome");
        }

        {
            let mut scoped = pool.scoped();
            scoped
                .try_push(loan_a)
                .as_ref()
                .expect("required fixture outcome");
            scoped
                .try_push(loan_b)
                .as_ref()
                .expect("required fixture outcome");
            assert_eq!(scoped.segments().len(), 2);
            let mut output = [0; 11];
            scatter::Reader::new(scoped.segments())
                .read_exact(&mut output)
                .as_ref()
                .expect("scatter reader must fill the exact retained byte length");
            assert_eq!(&output, b"firstsecond");
        }
    });
    assert_eq!(hot, [0; 4]);
}

#[test]
fn pool_retires_once() {
    const CAPACITY: usize = 4;
    const CHAINS: usize = 2;
    const BYTES: usize = 72 + CAPACITY * 32 + CHAINS * 40;

    let ((), lifetime) = Tracker::measure(|| {
        let last = {
            let result = segments::Pool::<storage::Shared>::try_new(segments::Plan {
                segments: CAPACITY,
                chains: CHAINS,
            });
            let pool = result.expect("the lifetime pool must allocate");
            let first = (pool.try_chain()).expect("retained edge must fit");
            let second = (pool.try_chain()).expect("retained edge must fit");
            assert_eq!(first.segments().len(), 0);
            second
        };
        assert_eq!(last.segments().len(), 0);
    });

    assert_eq!(lifetime, [1, BYTES, 1, BYTES]);
}

#[test]
fn segments_recycle() {
    let capacity = queue::Capacity::new(Count::new(8), Count::new(2));
    let bytes = 2 * mem::size_of::<storage::Shared>();
    let mut fifo =
        queue::Fifo::try_with(capacity).expect("the two-owner segment queue must allocate");

    let abc = (storage::Shared::try_copy(b"abc")).expect("the first segment owner must allocate");
    let def = (storage::Shared::try_copy(b"def")).expect("the second segment owner must allocate");
    let gh = (storage::Shared::try_copy(b"gh")).expect("the recycled segment owner must allocate");
    let first = (abc.try_clone()).expect("the first retained segment must fit");
    let second = (def.try_clone()).expect("the second retained segment must fit");
    let third = (gh.try_clone()).expect("the recycled retained segment must fit");
    let ((), hot) = Tracker::measure(|| {
        fifo.try_push(first)
            .as_ref()
            .expect("the first segment must fit");
        fifo.try_push(second)
            .as_ref()
            .expect("the second segment must fit");
        let () = (fifo.try_consume(4)).expect("the queued four-byte prefix must be consumable");
        fifo.try_push(third)
            .as_ref()
            .expect("the released segment slot must accept the recycled owner");

        let mut output = [0; 4];
        queue::Array::try_new(&fifo.cursor(), 0)
            .expect("retained four-byte suffix")
            .copy(&mut output);
        assert_eq!(output, *b"efgh");
        assert_eq!(fifo.segments(), 2);
    });
    assert_eq!(hot, [0; 4]);

    let ((), drops) = Tracker::measure(|| {
        let released = fifo;
        assert_eq!(released.len(), 4);
    });
    assert_eq!(drops, [0, 0, 1, bytes]);
    assert_eq!(
        (abc.as_slice(), def.as_slice(), gh.as_slice()),
        (&b"abc"[..], &b"def"[..], &b"gh"[..])
    );
}

#[test]
fn rejects_with_owner() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 0,
    }))
    .expect("the rejection pool must allocate");
    let first = (storage::Shared::try_copy(b"first")).expect("the accepted owner must allocate");
    let second = (storage::Shared::try_copy(b"second")).expect("the rejected owner must allocate");
    let ptr = second.as_slice().as_ptr();
    let mut chain = (pool.try_chain()).expect("metadata edge must fit");
    chain
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");

    let result = chain.try_push(second);
    assert!(result.is_err(), "the full chain must reject another owner");
    let Err(err) = result else { return };
    let (owner, cause) = err.into_parts();
    assert!(
        cause
            == segments::Error::ChainCapacity(capacity::Shortage {
                requested: NonZero::<usize>::MIN,
                available: 0
            })
    );
    assert_eq!(owner.as_slice().as_ptr(), ptr);
    assert!(chain.count() == segments::Count::ONE);
    assert_eq!(pool.available().segments, 1);
}

#[test]
fn releases_front_slot() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 3,
        chains: 1,
    }))
    .expect("the front-transfer pool must allocate");
    let first = (storage::Shared::try_copy(b"first")).expect("the transferred owner must allocate");
    let ptr = first.as_slice().as_ptr();
    let mut chain = (pool.try_chain()).expect("metadata edge must fit");
    chain
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");
    let second =
        (storage::Shared::try_copy(b"second")).expect("the retained tail owner must allocate");
    chain
        .try_push(second)
        .as_ref()
        .expect("metadata admits the owner");
    let third = (storage::Shared::try_copy(b"third")).expect("the final owner must allocate");
    chain
        .try_push(third)
        .as_ref()
        .expect("metadata admits the owner");

    let (chain,) = match chain.into_shape() {
        segments::Shape::Many(chain) => Some((chain,)),
        _ => None,
    }
    .expect("the fixture retains three segment owners");
    let (owner, chain) = chain.take_front();
    assert_eq!(owner.as_slice().as_ptr(), ptr);
    assert_eq!(owner.as_slice(), b"first");
    let (chain,) = match chain {
        segments::Nonempty::Many(chain) => Some((chain,)),
        segments::Nonempty::One(_) => None,
    }
    .expect("two remaining owners must keep their descriptor");
    assert!(
        chain
            .as_chain()
            .chunks()
            .take_bytes(usize::MAX)
            .flat_map(|chunk| chunk.bytes())
            .eq(b"secondthird")
    );
    assert_eq!(chain.as_chain().len(), 11);
    assert!(chain.as_chain().count() == segments::Count::TWO);
    assert_eq!(pool.available().segments, 1);
    assert_eq!(pool.available().chains, 0);
    let (owner, chain) = chain.take_front();
    assert_eq!(owner.as_slice(), b"second");
    let (owner,) = match chain {
        segments::Nonempty::One(owner) => Some((owner,)),
        segments::Nonempty::Many(_) => None,
    }
    .expect("the last owner must leave the descriptor inline");
    assert_eq!(owner.as_slice(), b"third");
    assert_eq!(pool.available().segments, 3);
    assert_eq!(pool.available().chains, 1);
}

#[test]
fn keeps_failed_rehome() {
    let spool = (segments::Pool::try_new(segments::Plan {
        segments: 2,
        chains: 1,
    }))
    .expect("the failed-rehome source pool must allocate");
    let tpool = (segments::Pool::try_new(segments::Plan {
        segments: 1,
        chains: 0,
    }))
    .expect("the undersized rehome target pool must allocate");
    let mut source = (spool.try_chain()).expect("metadata edge must fit");
    let first =
        (storage::Shared::try_copy(b"first")).expect("the failed-rehome head must allocate");
    let second =
        (storage::Shared::try_copy(b"second")).expect("the failed-rehome tail must allocate");
    source
        .try_push(first)
        .as_ref()
        .expect("metadata admits the owner");
    source
        .try_push(second)
        .as_ref()
        .expect("metadata admits the owner");
    let mut target = (tpool.try_chain()).expect("metadata edge must fit");

    assert!(matches!(
        source.try_move_to(&mut target, source.len()),
        Err(segments::Error::SegmentCapacity(capacity::Shortage {
            requested,
            available: 1
        })) if requested.get() == 2
    ));
    assert!(
        source
            .chunks()
            .take_bytes(usize::MAX)
            .flat_map(|chunk| chunk.bytes())
            .eq(b"firstsecond")
    );
    assert!(target.is_empty());
    assert_eq!(spool.available().segments, 0);
    assert_eq!(spool.available().chains, 0);
    assert_eq!(tpool.available().segments, 1);
    assert_eq!(tpool.available().chains, 0);
}

#[test]
fn keeps_failed_split() {
    let pool = (segments::Pool::try_new(segments::Plan {
        segments: 4,
        chains: 1,
    }))
    .expect("the failed-split pool must allocate");
    let mut chain = (pool.try_chain()).expect("metadata edge must fit");
    for value in [b"a".as_slice(), b"b", b"c", b"d"] {
        let owner =
            (storage::Shared::try_copy(value)).expect("each failed-split owner must allocate");
        chain
            .try_push(owner)
            .as_ref()
            .expect("metadata admits the owner");
    }

    assert!(matches!(
        chain.try_take(2),
        Err(segments::Error::ChainCapacity(capacity::Shortage {
            requested: NonZero::<usize>::MIN,
            available: 0
        }))
    ));
    assert!(
        chain
            .chunks()
            .take_bytes(usize::MAX)
            .flat_map(|chunk| chunk.bytes())
            .eq(b"abcd")
    );
    assert!(chain.count() == segments::Count::new(4));
}

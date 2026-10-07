pub mod common;

use std::{alloc::Layout, cell::Cell};

use common::alloc::{AlignedZst, Header};
use o3::alloc::{self, tail};

#[test]
fn repeats_copy() {
    let plan = tail::Plan::<Header, u16>::fixed::<3>();
    let header = Header(5);
    let memory = (plan.try_allocate()).expect("the fixed repeated tail block must allocate");
    let block = memory.repeat(header, 9);
    assert_eq!(tail::View::new(&block).header().0, 5);
    assert_eq!(tail::View::new(&block).as_slice(), &[9, 9, 9]);
}

#[test]
fn initializes_header() {
    let headers = Cell::new(Some(0));
    let entries = Cell::new(Some(0));
    let mut block = tail::Plan::<common::Counted<_>, common::Counted<_>>::fixed::<2>()
        .try_allocate()
        .expect("header and uninitialized tail")
        .init(common::Counted(&headers));
    assert_eq!(tail::View::new(&block).header().0.get(), Some(0));
    block.as_mut_slice()[0].write(common::Counted(&entries));
    drop(block);
    assert_eq!(headers.get(), Some(1));
    assert_eq!(entries.get(), Some(0));
}

#[test]
fn bounds_match_layout() {
    compare::<(), ()>();
    compare::<(), u8>();
    compare::<u8, u16>();
    compare::<u64, Header>();
    compare::<Header, u8>();
    compare::<Header, u16>();
    compare::<Header, Header>();
    compare::<AlignedZst, u16>();
    compare::<u8, AlignedZst>();
    compare::<AlignedZst, AlignedZst>();
    assert_eq!(
        tail::Plan::<(), ()>::fixed::<{ usize::MAX }>().size(),
        std::mem::size_of::<usize>()
    );
}

fn compare<H, T>() {
    let maximum = isize::MAX as usize;
    for len in [
        0,
        1,
        2,
        3,
        7,
        8,
        63,
        64,
        65,
        maximum / 64 - 3,
        maximum / 64,
        maximum / 2 - 3,
        maximum / 2,
        maximum - 8,
        maximum,
        usize::MAX,
    ] {
        let expected = Layout::new::<H>()
            .extend(Layout::new::<usize>())
            .and_then(|prefix| {
                let tail = Layout::array::<T>(len)?;
                prefix.0.pad_to_align().extend(tail)
            });
        match tail::Plan::<H, T>::try_new(len) {
            Ok(plan) => {
                let layout = expected.expect("accepted tail length must fit Layout");
                assert_eq!(plan.size(), layout.0.pad_to_align().size(), "length {len}");
            }
            Err(alloc::Overflow) => {
                std::assert_matches!(expected, Err(_), "rejected length {len}");
            }
        }
    }
}

#[test]
fn keeps_layout() {
    use std::mem;

    assert_eq!(
        mem::size_of::<tail::Plan<Header, u16>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<tail::Uninit<Header, u16>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<tail::Plan<u32, u32>>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<tail::Uninit<u32, u32>>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Result<tail::Plan<u32, u32>, alloc::Overflow>>(),
        2 * mem::size_of::<usize>()
    );
}

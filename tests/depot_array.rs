pub mod common;

use std::{cell::Cell, mem, ops, ptr};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{CompactBytes, model::drops},
};
use o3::depot::array::{self, Len, copy, init, sorted};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn copy_shape() {
    let copied = copy::Vec::from([3_u64, 5, 11, 13]);
    assert_eq!(copied.clone().as_slice(), [3, 5, 11, 13]);
    assert!(copied.extent().is_full());
    assert!(copy::Vec::<u64, 0>::new().is_empty());
    assert!(copy::Vec::from([(), ()]).extent().is_full());
    assert!(!mem::needs_drop::<copy::Vec<u64, 4>>());
    assert!(!mem::needs_drop::<copy::IntoIter<u64, 4>>());
    assert_eq!(
        mem::size_of::<copy::IntoIter<u64, 4>>(),
        mem::size_of::<copy::Vec<u64, 4>>()
    );
    assert_eq!(
        mem::size_of::<copy::Vec<u64, 4>>(),
        mem::size_of::<[u64; 4]>() + mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<copy::Vec<u8, 65>>(),
        mem::size_of::<CompactBytes>()
    );
}

#[test]
fn copies_prefix() {
    let mut source = copy::Vec::<u64, 4>::from_one(3);
    source.vacancy().expect("second position").insert(5);
    let mut target = copy::Vec::from([17, 19, 23, 29]);
    let ((), counts) = Tracker::measure(|| target.copy_from(&source));
    assert_eq!(counts, [0; 4]);
    assert_eq!(target.as_slice(), [3, 5]);
    assert!(target.extent() == source.extent());
    assert_eq!(source.as_slice(), [3, 5]);
    target.copy_from(&copy::Vec::new());
    assert!(target.is_empty());
    let mut empty = copy::Vec::<u64, 0>::new();
    empty.copy_from(&copy::Vec::new());
    assert!(empty.is_empty());
    let mut zero = copy::Vec::<(), 2>::new();
    zero.copy_from(&copy::Vec::from([(), ()]));
    assert!(zero.extent().is_full());
}

#[test]
fn length_overflow() {
    for actual in [3, usize::MAX] {
        let error = Len::<2>::try_from(actual).expect_err("oversized length");
        assert_eq!(
            error,
            array::Error {
                actual,
                capacity: 2
            }
        );
    }
    let len = Len::<2>::try_from(2).expect("boundary length");
    assert_eq!(len.prefix(&[1, 2]), &[1, 2]);
}

#[test]
fn copy_mutation_bounds() {
    let mut values = copy::Vec::<_, 4>::new();
    values.vacancy().expect("first position").insert(3);
    values.vacancy().expect("second position").insert(5);
    values.vacancy().expect("third position").insert(7);
    assert_eq!(values.as_slice(), [3, 5, 7]);
    values.vacancy().expect("last position").insert(9);
    assert!(values.extent().is_full());
    assert!(values.vacancy().is_none());
    assert_eq!(values.as_slice(), [3, 5, 7, 9]);

    values.as_mut_slice()[1] = 6;
    assert_eq!(values.pop(), Some(9));
    assert_eq!(values.as_slice(), [3, 6, 7]);
    assert_eq!(values.pop(), Some(7));
    assert_eq!(values.pop(), Some(6));
    assert_eq!(values.pop(), Some(3));
    assert_eq!(values.pop(), None);
    assert!(copy::Vec::<u8, 0>::new().vacancy().is_none());
    assert_eq!(copy::Vec::<u8, 0>::new().pop(), None);
}

#[test]
fn sorted_membership() {
    let ((), allocs) = Tracker::measure(|| {
        let mut values = sorted::Set::<_, 4>::new();
        for value in [5, 1, 9, 3] {
            values.try_insert(value).expect("unique member");
        }
        assert_eq!(
            values.try_insert(7),
            Err(sorted::Error::Full {
                value: 7,
                capacity: 4
            })
        );
        for value in [1, 3, 5, 9] {
            assert_eq!(
                values.try_insert(value),
                Err(sorted::Error::Duplicate {
                    value,
                    existing: value
                })
            );
        }
        assert_eq!(
            sorted::Set::<_, 0>::new().try_insert(1),
            Err(sorted::Error::Full {
                value: 1,
                capacity: 0
            })
        );
    });
    assert_eq!(allocs, [0; 4]);
    assert_eq!(
        mem::size_of::<sorted::Set<u64, 4>>(),
        mem::size_of::<copy::Vec<u64, 4>>()
    );
}

#[test]
fn selects_without_mutation() {
    let source = copy::Vec::from([7, 2, 5, 4]);
    let mut selected = copy::Vec::from([0; 4]);
    let ((), allocs) = Tracker::measure(|| {
        selected.filter_from(&source, |value| value % 2 == 0);
    });
    assert_eq!(allocs, [0; 4]);
    assert_eq!(source.as_slice(), [7, 2, 5, 4]);
    assert_eq!(selected.as_slice(), [2, 4]);
    let mut retained = source;
    retained.retain(|value| value % 2 == 0);
    assert_eq!(retained.as_slice(), selected.as_slice());
    let mut calls = 0;
    selected.filter_from(&retained, |_| {
        calls += 1;
        true
    });
    assert_eq!(calls, 2);
    assert_eq!(selected.as_slice(), retained.as_slice());
    selected.filter_from(&source, |_| false);
    assert!(selected.is_empty());
    calls = 0;
    selected.retain(|_| {
        calls += 1;
        true
    });
    assert_eq!(calls, 0);
    selected.filter_from(&source, |_| true);
    assert_eq!(selected.as_slice(), source.as_slice());
}

#[test]
fn mapping_retains_source() {
    let source = copy::Vec::from([3, 5, 8, 13]);
    let mut mapped = array::Vec::<_, 4>::new();
    let ((), allocs) = Tracker::measure(|| {
        mapped.map_from(&source, |slot, value| (slot.commit(), value));
    });
    assert_eq!(allocs, [0; 4]);
    for (index, (len, value)) in mapped.iter().enumerate() {
        assert_eq!(len.get(), index + 1);
        assert!(ptr::eq(*value, &source[index]));
    }
}

#[test]
fn mapping_releases_values() {
    let counts = [const { Cell::new(Some(0)) }; 2];
    let source = copy::Vec::from([0, 1]);
    {
        let mut mapped = array::Vec::from_fn(Len::MAX, |slot| Counted(&counts[slot.index()]));
        mapped.map_from(&source, |_, index| Counted(&counts[*index]));
        assert_eq!(counts.each_ref().map(Cell::get), [Some(1); 2]);
    }
    assert_eq!(counts.each_ref().map(Cell::get), [Some(2); 2]);
    let mut empty = array::Vec::<u8, 0>::new();
    let mut calls = 0;
    empty.map_from(&copy::Vec::new(), |_, value| {
        calls += 1;
        *value
    });
    assert_eq!(calls, 0);
    assert!(empty.is_empty());
}

#[test]
fn vec_drops_values() {
    let counts = [const { Cell::new(Some(0)) }; 2];
    {
        let values = array::Vec::from(counts.each_ref().map(Counted));
        assert_eq!(values.len(), 2);
    }
    assert_eq!(counts.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn default_is_empty() {
    let counts = [Cell::new(Some(0))];
    let ((), allocs) = Tracker::measure(|| {
        let mut values = array::Vec::<Counted<_>, 2>::default();
        assert!(values.is_empty());
        values
            .vacancy()
            .expect("empty vector")
            .insert(Counted(&counts[0]));
        {
            let owned = mem::take(&mut values);
            assert!(values.is_empty());
            assert_eq!(owned.len(), 1);
            assert_eq!(counts[0].get(), Some(0));
        }
        assert_eq!(counts[0].get(), Some(1));
        assert!(array::Vec::<Counted<&Cell<Option<usize>>>, 0>::default().is_empty());
    });
    assert_eq!(allocs, [0; 4]);
}

#[test]
fn len_proofs() {
    assert_eq!(Len::<4>::ZERO.get(), 0);
    assert_eq!(Len::<4>::ONE.get(), 1);
    assert_eq!(Len::<4>::MAX.get(), 4);
    assert_eq!(Len::<4>::exact::<3>().get(), 3);
    let mut values = [0; 256];
    let slot = array::Vacancy::<256>::at::<255>();
    *slot.get_mut(&mut values) = 7;
    assert_eq!(values[255], 7);
    assert_eq!(slot.commit().get(), 256);
    let last = array::Vacancy::<{ u32::MAX as usize }>::at::<{ u32::MAX as usize - 1 }>();
    assert_eq!(last.commit().get(), u32::MAX as usize);
    assert_eq!(mem::size_of::<array::Vacant<'_, u64, 4>>(), 16);
    assert_eq!(
        mem::size_of::<Option<array::Vacant<'_, u64, 4>>>(),
        mem::size_of::<array::Vacant<'_, u64, 4>>()
    );
}

#[test]
fn front_restores_owner() {
    let counts = [const { Cell::new(Some(0)) }; 2];
    let mut values = array::Vec::<_, 2>::from_fn(Len::MAX, |slot| {
        (slot.index(), Counted(&counts[slot.index()]))
    })
    .into_iter();
    let removed = values.remove_front().expect("front");
    assert_eq!(removed.sink().index(), 0);
    drop(removed);
    assert_eq!(o3::depot::raw::Exact::remaining(&values).get(), 2);
    let (slot, value) = values.remove_front().expect("restored front").into_parts();
    assert_eq!((slot.index(), value.0), (0, 0));
    drop(values);
    let mut source = array::Vec::from([value]).into_iter();
    let borrowed = &mut source;
    let value = borrowed.next().expect("borrowed retained item");
    assert!(borrowed.next().is_none());
    assert_eq!(o3::depot::raw::Exact::remaining(&borrowed).get(), 0);
    // SAFETY: this exact loan just reached None without an intervening mutation.
    unsafe { o3::depot::raw::Exact::finish(borrowed) };
    assert!(source.next().is_none());
    drop(source);
    assert_eq!(counts.each_ref().map(Cell::get), [0, 1].map(Some));
    drop(value);
    assert_eq!(counts.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn guard_release_order() {
    let order = Cell::new(Some(0));
    let mut backing = mem::MaybeUninit::uninit();
    assert!(matches!(
        drops::Order::fill(init::Guard::<_, 3, false>::new(&mut backing), &order, true),
        Err(3)
    ));
    assert_eq!(order.get(), Some(12));
    order.set(Some(0));
    assert!(matches!(
        drops::Order::fill(init::Guard::<_, 3, true>::new(&mut backing), &order, true),
        Err(3)
    ));
    assert_eq!(order.get(), Some(21));
    order.set(Some(0));
    {
        let values =
            drops::Order::fill(init::Guard::<_, 3, true>::new(&mut backing), &order, false)
                .expect("complete array");
        assert_eq!(values.each_ref().map(|value| value.1), [1, 2, 3]);
        assert_eq!(order.get(), Some(0));
    }
    assert_eq!(order.get(), Some(123));
}

#[test]
fn guard_large_zst() {
    const N: usize = u32::MAX as usize + 1;
    let mut backing = mem::MaybeUninit::<[(); N]>::uninit();
    let vacant = init::Guard::<_, N, true>::new(&mut backing)
        .next()
        .continue_value()
        .expect("nonempty backing array");
    assert!(vacant.insert(()).next().is_continue());
}

#[test]
fn guard_maps_pairs() {
    let counts = [const { Cell::new(Some(0)) }; 2];
    let mut backing = mem::MaybeUninit::uninit();
    let mut guard = init::Guard::<_, 2, true>::new(&mut backing);
    let full = loop {
        let vacant = match guard.next() {
            ops::ControlFlow::Break(full) => break full,
            ops::ControlFlow::Continue(vacant) => vacant,
        };
        let value = Counted(vacant.get(&counts));
        guard = vacant.insert(value);
    };
    let mut calls = 0;
    let mapped = full.map_with([7, 11], |value, pair| {
        assert!(ptr::eq(value.0, &counts[calls]));
        calls += 1;
        if calls == 1 {
            drop(value);
            Err(pair)
        } else {
            Ok((pair, value))
        }
    });
    assert!(matches!(&mapped, [Err(7), Ok((11, _))]));
    assert_eq!(counts.each_ref().map(Cell::get), [1, 0].map(Some));
    drop(mapped);
    assert_eq!(counts.each_ref().map(Cell::get), [Some(1); 2]);
}

#[test]
fn lifecycle_no_alloc() {
    let ((), allocs) = Tracker::measure(|| {
        let mut owned =
            array::Vec::<u64, 4>::from_fn(Len::exact::<2>(), |slot| slot.index() as u64 + 1);
        assert_eq!(owned.push(3), Ok(()));
        let mut iter = owned.into_iter();
        assert_eq!(iter.next(), Some(1));
        assert!(iter.eq([2, 3]));

        let copied = copy::Vec::from([3_u64, 5]);
        assert_eq!(copied.into_iter().sum::<u64>(), 8);

        let mut backing = mem::MaybeUninit::uninit();
        let mut guard = init::Guard::<_, 4, false>::new(&mut backing);
        let filled = loop {
            let vacant = match guard.next() {
                ops::ControlFlow::Break(values) => {
                    break values.map_with([0, 1, 2, 3], |(), value| value);
                }
                ops::ControlFlow::Continue(vacant) => vacant,
            };
            guard = vacant.insert(());
        };
        assert_eq!(filled, [0, 1, 2, 3]);
        let mut backing = mem::MaybeUninit::<[(); 0]>::uninit();
        let empty = init::Guard::<_, 0>::new(&mut backing)
            .next()
            .break_value()
            .expect("empty array");
        let mut calls = 0;
        let mapped = empty.map_with([], |(), ()| {
            calls += 1;
            calls
        });
        assert_eq!((mapped, calls), ([], 0));
    });
    assert_eq!(allocs, [0; 4]);
}

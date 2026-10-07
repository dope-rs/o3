pub mod common;

use std::{cell::Cell, mem};

use common::{
    Counted,
    alloc::system::Tracker,
    depot::{Fault, model::drops},
};
use o3::depot::array::{self, IntoIter, Len, init};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn guard_restores_prefix() {
    let counts = [const { Cell::new(Some(0)) }; 3];
    let mut values = counts.each_ref().map(|count| Some(Counted(count)));
    let mut backing = mem::MaybeUninit::uninit();
    let mut guard = init::Guard::<_, 3>::new(&mut backing);
    for _ in 0..2 {
        let vacant = guard.next().continue_value().expect("remaining position");
        let value = vacant.get_mut(&mut values).take().expect("owned value");
        guard = vacant.insert(value);
    }
    assert!(values[0].is_none() && values[1].is_none());
    for (value, slot) in guard.into_iter().zip(&mut values) {
        *slot = Some(value);
    }
    assert!(values.iter().all(Option::is_some));
    assert_eq!(counts.each_ref().map(Cell::get), [Some(0); 3]);
    drop(values);
    assert_eq!(counts.each_ref().map(Cell::get), [Some(1); 3]);
}

#[test]
fn guard_drains_prefix() {
    let order = Cell::new(Some(0));
    let mut backing = mem::MaybeUninit::uninit();
    let mut guard = init::Guard::<_, 4>::new(&mut backing);
    for value in 1..=3 {
        guard = guard
            .next()
            .continue_value()
            .expect("remaining position")
            .insert(drops::Order(&order, value));
    }
    let mut values = guard.into_iter();
    let first = values.next().expect("initialized prefix");
    assert_eq!((first.1, values.len()), (1, 2));
    values.as_mut_slice()[0].1 = 4;
    drop(values);
    assert_eq!(order.get(), Some(43));
    drop(first);
    assert_eq!(order.get(), Some(431));

    order.set(Some(0));
    let mut guard = init::Guard::<_, 4, true>::new(&mut backing);
    for value in 1..=3 {
        guard = guard
            .next()
            .continue_value()
            .expect("remaining position")
            .insert(drops::Order(&order, value));
    }
    let mut values = guard.into_iter();
    let first = values.next().expect("initialized prefix");
    assert_eq!((first.1, values.len()), (1, 2));
    drop(values);
    assert_eq!(order.get(), Some(32));
    drop(first);
    assert_eq!(order.get(), Some(321));
}

#[test]
fn prefix_reuses_backing() {
    use common::alloc::Owner;
    use o3::alloc::BoxArrayExt as _;

    let orders = [const { Cell::new(Some(0)) }; 2];
    let mut backing = Box::<[mem::MaybeUninit<Owner<'static>>; 3]>::try_uninit()
        .expect("reusable prefix backing");
    let pointer = backing.as_ptr().cast::<()>();
    for (index, order) in orders.iter().enumerate() {
        let (result, activity) = Tracker::measure(|| {
            init::Prefix::<Owner<'_>, 3>::with(&mut backing, |prefix| {
                assert!(prefix.as_slice().is_empty());
                let first = prefix.vacancy().expect("first position").insert(Owner {
                    drops: order,
                    digit: 1,
                });
                let second = prefix.vacancy().expect("second position").insert(Owner {
                    drops: order,
                    digit: 2,
                });
                assert_eq!(prefix.get(first).digit, 1);
                prefix.get_mut(second).digit = 3;
                assert_eq!(prefix.as_slice().as_ptr().cast::<()>(), pointer);
                if index == 0 {
                    return Err(Fault::Element(2));
                }
                prefix.vacancy().expect("last position").insert(Owner {
                    drops: order,
                    digit: 4,
                });
                assert!(prefix.vacancy().is_none());
                Ok(prefix.as_slice().len())
            })
        });
        assert_eq!(activity, [0; 4]);
        assert_eq!(
            result,
            if index == 0 {
                Err(Fault::Element(2))
            } else {
                Ok(3)
            }
        );
        assert_eq!(order.get(), Some(if index == 0 { 13 } else { 134 }));
    }
    drop(backing);
    assert_eq!(orders.each_ref().map(Cell::get), [Some(13), Some(134)]);
}

#[test]
fn scoped_prefix_capacity() {
    let mut empty: [mem::MaybeUninit<u8>; 0] = [];
    init::Prefix::<u8, 0>::with(&mut empty, |prefix| {
        assert!(prefix.vacancy().is_none());
        assert!(prefix.as_slice().is_empty());
    });
    let mut backing = [mem::MaybeUninit::<u8>::uninit(); 256];
    init::Prefix::<u8, 256>::with(&mut backing, |prefix| {
        for value in u8::MIN..=u8::MAX {
            let key = prefix.vacancy().expect("bounded position").insert(value);
            assert_eq!(*key.as_inner(), value);
            assert_eq!(*prefix.get(key), value);
        }
        assert!(prefix.vacancy().is_none());
        assert_eq!(prefix.as_slice().len(), 256);
    });
}

#[test]
fn mapped_prefix_reuses() {
    use o3::alloc::BoxArrayExt as _;

    let mut input = [const { mem::MaybeUninit::<Cell<Option<usize>>>::uninit() }; 4];
    let mut backing =
        Box::<[mem::MaybeUninit<[usize; 2]>; 4]>::try_uninit().expect("reusable mapping backing");
    let pointer = backing.as_ptr().cast::<()>();
    let ((), activity) = Tracker::measure(|| {
        init::Prefix::<Cell<Option<usize>>, 4>::with(&mut input, |source| {
            for _ in 0..4 {
                source
                    .vacancy()
                    .expect("source position")
                    .insert(Cell::new(Some(0)));
            }
            let mut next = 0;
            let (guard, error) = init::Guard::<_, 4>::try_map(&mut backing, source, |value| {
                let index = next;
                next += 1;
                match index {
                    1 => Ok(None),
                    3 => Err(Fault::Element(3)),
                    _ => Ok(Some(Counted(value))),
                }
            })
            .err()
            .expect("partial mapping");
            assert_eq!(error, Fault::Element(3));
            assert_eq!(guard.as_slice().len(), 2);
            assert_eq!(guard.as_slice().as_ptr().cast::<()>(), pointer);
            assert!(std::ptr::eq(guard.as_slice()[1].0, &source.as_slice()[2]));
            assert!(source.as_slice().iter().map(Cell::get).eq([Some(0); 4]));
            let mut values = guard.into_iter();
            let first = values.next().expect("first mapped value");
            drop(values);
            assert!(source.as_slice().iter().map(Cell::get).eq([
                Some(0),
                Some(0),
                Some(1),
                Some(0)
            ]));
            drop(first);

            let guard = init::Guard::<_, 4>::try_map(&mut backing, source, |value| {
                Ok::<_, Fault>(Some(Counted(value)))
            })
            .ok()
            .expect("complete mapping");
            assert_eq!(guard.as_slice().as_ptr().cast::<()>(), pointer);
            drop(guard);
            assert!(source.as_slice().iter().map(Cell::get).eq([
                Some(2),
                Some(1),
                Some(2),
                Some(1)
            ]));
        });
    });
    assert_eq!(activity, [0; 4]);
}

#[test]
fn iter_range_and_drop() {
    assert_eq!(
        mem::size_of::<array::Vec<u64, 4>>(),
        5 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<IntoIter<u64, 4>>(),
        mem::size_of::<array::Vec<u64, 4>>()
    );

    let counts = [const { Cell::new(Some(0)) }; 4];
    let mut values =
        array::Vec::<_, 4>::from_fn(Len::MAX, |slot| Counted(&counts[slot.index()])).into_iter();
    assert!(values.next().is_some());
    assert_eq!(values.count(), 3);
    assert_eq!(counts.each_ref().map(Cell::get), [Some(1); 4]);

    let mut values = array::Vec::<_, 4>::from_fn(Len::MAX, |slot| Counted(&counts[slot.index()]));
    {
        let mut drained = values.drain();
        drop(drained.next().expect("first value"));
    }
    assert_eq!(counts.each_ref().map(Cell::get), [Some(2); 4]);
    assert_eq!(values.extent().get(), 0);
    values
        .vacancy()
        .expect("drained position")
        .insert(Counted(&counts[0]));
    drop(values);
    assert_eq!(counts.each_ref().map(Cell::get), [3, 2, 2, 2].map(Some));
}

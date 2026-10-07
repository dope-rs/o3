pub mod common;

use std::{cell, mem, ptr};

use common::alloc::{AlignedZst, owner::Owner, system::Tracker};
use o3::alloc::{self, BoxExt, Error, Gap, StrExt, Vacant, VecExt};

#[global_allocator]
static ALLOCATOR: Tracker = Tracker;

#[test]
fn gap_preserves_owners() {
    for index in 0..=3 {
        let drops = cell::Cell::new(Some(0));
        {
            let mut values = Vec::try_new(4).expect("four owner slots");
            for value in [1, 3, 5] {
                values.push(common::Owner {
                    drops: common::Counted(&drops),
                    value,
                });
            }
            let pointer = values.as_ptr();
            let ((), activity) = Tracker::measure(|| {
                let gap = Gap::partition(&mut values, |value| value.value < 2 * index);
                assert_eq!(gap.index(), index);
                assert!(
                    gap.tail()
                        .iter()
                        .map(|value| value.value)
                        .eq([1, 3, 5].into_iter().skip(index))
                );
                assert!(
                    gap.try_insert(common::Owner {
                        drops: common::Counted(&drops),
                        value: 2 * index,
                    })
                    .is_ok()
                );
                assert_eq!(values[index].value, 2 * index);
                assert!(values.windows(2).all(|pair| pair[0].value < pair[1].value));
                let rejected = Gap::partition(&mut values, |_| true)
                    .try_insert(common::Owner {
                        drops: common::Counted(&drops),
                        value: 7,
                    })
                    .expect_err("full vector returns ownership");
                assert_eq!(rejected.value, 7);
                assert_eq!(drops.get(), Some(0));
                drop(rejected);
            });
            assert_eq!(activity, [0; 4]);
            assert_eq!(values.as_ptr(), pointer);
            assert_eq!(values.len(), 4);
            assert_eq!(drops.get(), Some(1));
        }
        assert_eq!(drops.get(), Some(5));
    }
}

#[test]
fn gap_abandonment() {
    let mut values = Vec::<u16>::new();
    assert_eq!(Gap::partition(&mut values, |_| true).try_insert(7), Err(7));
    values.try_grow(2).expect("two slots");
    assert!(Gap::partition(&mut values, |_| false).try_insert(3).is_ok());
    assert_eq!(Gap::partition(&mut values, |_| false).index(), 0);
    assert_eq!(values, [3]);
}

#[test]
fn gap_zero_sized_limit() {
    let mut values = vec![(); usize::MAX - 1];
    assert!(Gap::partition(&mut values, |_| true).try_insert(()).is_ok());
    assert_eq!(values.len(), usize::MAX);
    assert_eq!(
        Gap::partition(&mut values, |_| false).try_insert(()),
        Err(())
    );
    assert_eq!(values.len(), usize::MAX);
}

#[test]
fn vec_owner() {
    let source = cell::Cell::new(7);
    let (built, activity) = Tracker::measure(|| Vec::<&cell::Cell<u64>>::try_new(3));
    let mut values = built.expect("borrowed vector allocation");
    let bytes = 3 * mem::size_of::<&cell::Cell<u64>>();
    assert_eq!(activity, [1, bytes, 0, 0]);
    assert_eq!(values.capacity(), 3);
    values.extend([&source; 3]);
    source.set(29);
    let ((), activity) = Tracker::measure(move || {
        assert!(values.iter().all(|value| value.get() == 29));
    });
    assert_eq!(activity, [0, 0, 1, bytes]);
}

#[test]
fn vec_admit() {
    let (overflow, activity) =
        Tracker::measure(|| Tracker::fail_after(0, || Vec::<u16>::try_new(usize::MAX)));
    assert!(matches!(overflow, Err(alloc::Error::Overflow)));
    assert_eq!(activity, [0; 4]);
    let (failed, activity) = Tracker::measure(|| Tracker::fail_after(0, || Vec::<u16>::try_new(3)));
    assert!(matches!(failed, Err(alloc::Error::Exhausted)));
    assert_eq!(activity, [0; 4]);
    let (empty, activity) = Tracker::measure(|| Tracker::fail_after(0, || Vec::<u64>::try_new(0)));
    let empty = empty.expect("empty vector needs no allocation");
    assert_eq!(activity, [0; 4]);
    assert_eq!(empty.capacity(), 0);
    assert_eq!(empty.as_ptr().addr() % mem::align_of::<u64>(), 0);
}

#[test]
fn reserves_geometrically() {
    let mut values = Vec::<u16>::new();
    VecExt::try_reserve(&mut values, 1).expect("initial reservation fits");
    let first = values.capacity();
    assert!(first >= 1);
    values.extend(0..first as u16);
    let pointer = values.as_ptr();
    VecExt::try_reserve(&mut values, 0).expect("existing capacity fits");
    assert_eq!(values.as_ptr(), pointer);
    VecExt::try_reserve(&mut values, 1).expect("growth fits");
    assert_eq!(values.capacity(), 2 * first);
    assert!(values.iter().copied().eq(0..first as u16));
}

#[test]
fn plans_before_allocation() {
    use alloc::array::Plan;

    let (plan, activity) = Tracker::measure(|| Plan::<u16, usize>::try_reserve(4, 5));
    assert_eq!(activity, [0; 4]);
    let plan = plan.expect("geometric layout");
    let values = plan.try_vec().expect("planned vector allocation");
    assert_eq!(values.capacity(), 8);
    assert!(values.is_empty());
    assert_eq!(
        *Plan::<u16, usize>::try_reserve(4, 3)
            .expect("existing capacity")
            .extent(),
        4
    );
    assert_eq!(
        *Plan::<u16, usize>::try_reserve(4, 13)
            .expect("required extent")
            .extent(),
        13
    );
    assert!(matches!(
        Plan::<u16, usize>::try_reserve(0, usize::MAX),
        Err(alloc::Overflow)
    ));
}

#[test]
fn reserves_zero_sized() {
    let mut values = Vec::<()>::new();
    VecExt::try_reserve(&mut values, usize::MAX).expect("zero sized elements need no allocation");
    values.push(());
    assert_eq!(
        VecExt::try_reserve(&mut values, usize::MAX),
        Err(Error::Overflow)
    );
    assert_eq!(values.len(), 1);
}

#[test]
fn plans_empty_string() {
    use alloc::array::Plan;

    let plan = Plan::try_new(7usize).expect("string capacity");
    assert!(matches!(
        Tracker::fail_after(0, || plan.try_string()),
        Err(alloc::Exhausted)
    ));
    let (value, activity) = Tracker::measure(|| plan.try_string());
    let value = value.expect("empty UTF-8 allocation");
    assert_eq!(activity, [1, 7, 0, 0]);
    assert_eq!(value.capacity(), 7);
    assert!(value.is_empty());
}

#[test]
fn owns_utf8_string() {
    let source = "한글 🦀";
    assert_eq!(
        Tracker::fail_after(0, || source.try_to_owned()),
        Err(alloc::Exhausted)
    );
    let owned = source.try_to_owned().expect("UTF-8 string allocation");
    assert_eq!(owned.as_bytes(), source.as_bytes());
}

#[test]
fn growth_keeps_owners() {
    for reserve in [false, true] {
        let drops = cell::Cell::new(Some(0));
        {
            let mut values = Vec::<Owner<'_>>::try_new(2).expect("two owner slots");
            for value in [1, 2] {
                values.push(Owner {
                    drops: &drops,
                    value: Box::new(value),
                });
            }
            let pointer = values.as_ptr();
            let (failed, activity) = Tracker::measure(|| {
                Tracker::fail_after(0, || {
                    if reserve {
                        VecExt::try_reserve(&mut values, 1)
                    } else {
                        values.try_grow(3)
                    }
                })
            });
            assert_eq!(failed, Err(Error::Exhausted));
            assert_eq!(activity, [0; 4]);
            assert_eq!(values.as_ptr(), pointer);
            assert_eq!(values.capacity(), 2);
            assert!(values.iter().map(|owner| *owner.value).eq([1, 2]));
            assert_eq!(drops.get(), Some(0));
            if reserve {
                VecExt::try_reserve(&mut values, 1).expect("geometric growth");
            } else {
                values.try_grow(3).expect("exact growth");
            }
            assert_eq!(values.capacity(), if reserve { 4 } else { 5 });
            assert!(values.iter().map(|owner| *owner.value).eq([1, 2]));
            let overflow = if reserve {
                VecExt::try_reserve(&mut values, usize::MAX)
            } else {
                values.try_grow(usize::MAX)
            };
            assert_eq!(overflow, Err(Error::Overflow));
            assert_eq!(values.capacity(), if reserve { 4 } else { 5 });
            assert!(values.iter().map(|owner| *owner.value).eq([1, 2]));
            assert_eq!(drops.get(), Some(0));
        }
        assert_eq!(drops.get(), Some(12));
    }
}

#[test]
fn vacancy_keeps_owner() {
    let drops = cell::Cell::new(Some(0));
    {
        let mut values = Vec::try_new(2).expect("two owner slots");
        values.push(common::Counted(&drops));
        let pointer = values.as_ptr();
        let ((), activity) = Tracker::measure(|| {
            assert!(Vacant::try_new(&mut values).is_ok());
            assert_eq!(values.len(), 1);
            Vacant::try_new(&mut values)
                .expect("existing slot")
                .insert(common::Counted(&drops));
        });
        assert_eq!(activity, [0; 4]);
        assert_eq!(values.as_ptr(), pointer);
        assert_eq!(values.len(), 2);
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn vacancy_keeps_failures() {
    let drops = cell::Cell::new(Some(0));
    {
        let mut values = Vec::try_new(1).expect("one owner slot");
        values.push(Owner {
            drops: &drops,
            value: Box::new(1),
        });
        let pointer = values.as_ptr();
        let (error, activity) =
            Tracker::measure(|| Tracker::fail_after(0, || Vacant::try_new(&mut values).err()));
        assert_eq!(error, Some(Error::Exhausted));
        assert_eq!(activity, [0; 4]);
        assert_eq!(values.as_ptr(), pointer);
        assert_eq!(values.capacity(), 1);
        let failed: Result<(), alloc::Error> = Tracker::fail_after(1, || {
            let vacant = Vacant::try_new(&mut values)?;
            let value = BoxExt::try_new(2)?;
            vacant.insert(Owner {
                drops: &drops,
                value,
            });
            Ok(())
        });
        assert_eq!(failed, Err(Error::Exhausted));
        assert!(values.capacity() > 1);
        assert_eq!(values.len(), 1);
        assert_eq!(*values[0].value, 1);
        assert_eq!(drops.get(), Some(0));
    }
    assert_eq!(drops.get(), Some(1));
}

#[test]
fn vacancy_zero_sized() {
    let mut values = vec![(); usize::MAX - 1];
    let (error, activity) = Tracker::measure(|| {
        Vacant::try_new(&mut values)
            .expect("last zero-sized slot")
            .insert(());
        Vacant::try_new(&mut values).err()
    });
    assert_eq!(values.len(), usize::MAX);
    assert_eq!(error, Some(Error::Overflow));
    assert_eq!(activity, [0; 4]);
}

#[test]
fn swap_preserves_owners() {
    let drops = cell::Cell::new(Some(0));
    let mut values = Vec::try_new(3).expect("three owners");
    for value in [1, 2, 3] {
        values.push(common::Owner {
            drops: common::Counted(&drops),
            value,
        });
    }
    let pointer = values.as_ptr();
    let capacity = values.capacity();
    let (removed, counts) = Tracker::measure(|| {
        // SAFETY: index one is initialized in this exclusively borrowed three-element vector.
        let (first, moved) = unsafe { alloc::raw::VecExt::swap_remove(&mut values, 1) };
        let moved = moved.expect("tail moves into the middle");
        assert_eq!(moved.value, 3);
        assert_eq!(ptr::from_ref(moved), pointer.wrapping_add(1));
        // SAFETY: index one is the initialized tail of the same shortened vector.
        let (second, moved) = unsafe { alloc::raw::VecExt::swap_remove(&mut values, 1) };
        assert!(moved.is_none());
        // SAFETY: index zero is the same vector's sole remaining initialized element.
        let (last, moved) = unsafe { alloc::raw::VecExt::swap_remove(&mut values, 0) };
        assert!(moved.is_none());
        [first, second, last]
    });
    assert_eq!(counts, [0; 4]);
    assert_eq!(removed.each_ref().map(|owner| owner.value), [2, 3, 1]);
    assert!(values.is_empty());
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.capacity(), capacity);
    drop(values);
    assert_eq!(drops.get(), Some(0));
    drop(removed);
    assert_eq!(drops.get(), Some(3));
}

#[test]
fn swap_zero_sized() {
    let mut values = Vec::from([AlignedZst; usize::MAX]);
    let ((), counts) = Tracker::measure(|| {
        // SAFETY: index zero is initialized in this exclusively borrowed nonempty vector.
        let (_, moved) = unsafe { alloc::raw::VecExt::swap_remove(&mut values, 0) };
        let moved = moved.expect("nonterminal zero-sized removal");
        assert_eq!(
            ptr::from_ref(moved).addr() % mem::align_of::<AlignedZst>(),
            0
        );
        // SAFETY: the first removal left usize::MAX - 1 initialized elements in this vector.
        let (_, moved) = unsafe { alloc::raw::VecExt::swap_remove(&mut values, usize::MAX - 2) };
        assert!(moved.is_none());
    });
    assert_eq!(values.len(), usize::MAX - 2);
    assert_eq!(counts, [0; 4]);
}

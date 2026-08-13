use std::{cell::Cell, mem};

use o3::collections::batch;

#[test]
fn fill_requires_an_empty_batch() {
    let mut values = batch::Inline::<u32, 2>::new();
    let mut fill = values.fill().expect("a new batch is empty");
    fill.vacant_entry().unwrap().insert(1);
    fill.vacant_entry().unwrap().insert(2);
    assert!(fill.vacant_entry().is_none());
    drop(fill);

    assert!(values.fill().is_none());
    assert_eq!(values.pop_front(), Some(1));
    assert!(values.fill().is_none());
    assert_eq!(values.pop_front(), Some(2));

    let mut fill = values.fill().expect("a drained batch can be refilled");
    fill.vacant_entry().unwrap().insert(3);
    assert_eq!(values.pop_front(), Some(3));
}

#[test]
fn reserved_pop_can_restore_the_original_front() {
    let mut values = batch::Inline::<u32, 2>::new();
    let mut fill = values.fill().unwrap();
    fill.vacant_entry().unwrap().insert(1);
    fill.vacant_entry().unwrap().insert(2);
    drop(fill);

    let (value, vacant) = values.pop_front_reserved().unwrap();
    vacant.restore(value);
    assert_eq!(values.pop_front(), Some(1));
    assert_eq!(values.pop_front(), Some(2));
}

#[test]
fn dropping_a_reserved_vacancy_commits_the_pop() {
    let mut values = batch::Inline::<u32, 2>::new();
    let mut fill = values.fill().unwrap();
    fill.vacant_entry().unwrap().insert(1);
    fill.vacant_entry().unwrap().insert(2);
    drop(fill);

    let (value, vacant) = values.pop_front_reserved().unwrap();
    assert_eq!(value, 1);
    std::hint::black_box(vacant);
    assert_eq!(values.pop_front(), Some(2));
}

#[test]
fn live_values_drop_exactly_once() {
    struct Count<'a>(&'a Cell<usize>);

    impl Drop for Count<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    let drops = Cell::new(0);
    let mut values = batch::Inline::<Count<'_>, 3>::new();
    let mut fill = values.fill().unwrap();
    fill.vacant_entry().unwrap().insert(Count(&drops));
    fill.vacant_entry().unwrap().insert(Count(&drops));
    fill.vacant_entry().unwrap().insert(Count(&drops));
    drop(fill);

    drop(values.pop_front());
    drop(values);
    assert_eq!(drops.get(), 3);
}

#[test]
fn inline_layout_is_payload_plus_two_indices() {
    assert_eq!(
        mem::size_of::<batch::Inline<u64, 3>>(),
        3 * mem::size_of::<u64>() + 2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<batch::Fill<'static, u64, 3>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<batch::Vacant<'static, u64, 3>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<batch::FrontVacant<'static, u64, 3>>(),
        mem::size_of::<usize>()
    );
}

pub mod common;

use std::{
    cell::Cell,
    marker,
    mem::{self, MaybeUninit},
    ptr,
};

use common::{Counted, Owner, mem::Pinned};
use o3::mem::slot::Vacant;

#[test]
fn retains_or_forgets_owner() {
    let drops = Cell::new(Some(0));
    let mut backing = MaybeUninit::uninit();
    let occupied = Vacant::new(&mut backing).insert(Owner {
        drops: Counted(&drops),
        value: 42,
    });
    assert_eq!(occupied.value, 42);
    let value = occupied.into_inner();
    assert_eq!(drops.get(), Some(0));
    mem::forget(Vacant::new(&mut backing).insert(Owner {
        drops: Counted(&drops),
        value: 1,
    }));
    assert_eq!(drops.get(), Some(0));
    let replaced = Vacant::new(&mut backing).insert(Owner {
        drops: Counted(&drops),
        value: 2,
    });
    assert_eq!(replaced.value, 2);
    assert_eq!(value.value, 42);
    drop(value);
    assert_eq!(drops.get(), Some(1));
    drop(replaced);
    assert_eq!(drops.get(), Some(2));
}

#[test]
fn pins_until_drop() {
    let drops = Cell::new(Some(0));
    let inputs = Cell::new(Some(0));
    let input = Owner {
        drops: Counted(&inputs),
        value: 42,
    };
    let mut backing = MaybeUninit::uninit();
    let result = Vacant::new(&mut backing)
        .insert(Pinned {
            address: Cell::new(ptr::null()),
            drops: Counted(&drops),
            pin: marker::PhantomPinned,
        })
        .with_pin(|value| {
            let input = input;
            value.address.set(ptr::from_ref(value.as_ref().get_ref()));
            assert_eq!(drops.get(), Some(0));
            Err::<(), _>(input.value)
        });
    assert_eq!(result, Err(42));
    assert_eq!(drops.get(), Some(1));
    assert_eq!(inputs.get(), Some(1));
}

#[test]
fn storage_retains_value() {
    use o3::mem::slot::Storage;

    let drops = Cell::new(Some(0));
    let mut storage = Storage::new(7_u8);
    let mut slot = storage.vacant().expect("empty storage").insert(Owner {
        drops: Counted(&drops),
        value: 42,
    });
    let pointer = ptr::from_ref(slot.get());
    slot.get_mut().value = 43;
    assert_eq!(ptr::from_ref(slot.get()), pointer);
    let value = slot.into_mut();
    assert_eq!(ptr::from_ref(&*value), pointer);
    value.value = 44;
    assert_eq!(storage.occupied().expect("borrowed owner").get().value, 44);
    let value = storage.occupied().expect("owner after borrow").take().0;
    assert!(storage.get().is_none());
    assert_eq!(drops.get(), Some(0));
    drop(value);
    assert_eq!(drops.get(), Some(1));

    let (value, vacant) = storage
        .vacant()
        .expect("returned capacity")
        .insert(Owner {
            drops: Counted(&drops),
            value: 47,
        })
        .take();
    let slot = vacant.insert(value);
    assert_eq!(drops.get(), Some(1));
    slot.clear();
    assert_eq!(*storage.metadata(), 7);
    assert!(storage.get().is_none());
    assert_eq!(drops.get(), Some(2));
    drop(storage);
    assert_eq!(drops.get(), Some(2));
}

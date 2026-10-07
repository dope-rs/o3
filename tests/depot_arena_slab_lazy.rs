use std::{pin::pin, ptr};

use o3::depot::{
    arena::slab::{self, storage},
    capacity::Count,
    index::Initialized,
    raw::Proof,
};

#[test]
fn rollback_before_growth() {
    type Store = slab::Pinned<u8, (), 2, false, 2, storage::Heap, storage::Lazy>;
    let mut store = pin!(Store::try_new(Count::new(2)).expect("two slots"));
    let first = store.as_mut().vacant().expect("first reservation");
    let index = first.index();
    drop(first);

    let first = store.as_mut().vacant().expect("returned reservation");
    assert_eq!(first.index(), index);
    let (key, value) = first.occupy(7);
    let retained = ptr::from_ref(value.as_ref().get_ref());
    let second = store.as_mut().vacant().expect("second reservation");
    assert_ne!(second.index(), index);
    second.insert(9);
    // SAFETY: this index remains occupied in this exact owner across the second slot's growth.
    let live = unsafe { Proof::<_, Initialized<()>>::new((store.as_ref().get_ref(), index)) };
    let value = live.into();
    assert_eq!(*value, 7);
    assert!(ptr::eq(value, retained));
    assert!(store.as_mut().vacant().is_none());

    // SAFETY: the same initialized index remains under this owner's exclusive pinned loan.
    let live = unsafe { Proof::<_, Initialized<()>>::new((store.as_mut(), index)) };
    let mut entry = live.into();
    assert!(ptr::eq(entry.value().as_ref().get_ref(), retained));
    *entry.value().get_mut() = 13;
    assert_eq!(*entry.get(), 13);
    entry.discard();
    assert!(store.parts(key.parts()).is_none());
    let vacant = store.as_mut().vacant().expect("discarded slot");
    assert_eq!(vacant.index(), index);
    let next = vacant.insert(11);
    assert_eq!(next.generation().get(), key.generation().get() + 1);
    assert!(store.parts(key.parts()).is_none());
    assert_eq!(store.parts(next.parts()), Some(&11));
    store
        .as_mut()
        .anchor(next.parts())
        .expect("live slot")
        .discard();
    assert!(store.parts(next.parts()).is_none());
    assert!(store.as_mut().vacant().is_none());
}

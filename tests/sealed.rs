use std::{collections::BTreeSet, marker, mem, ptr};

use o3::collections::{
    batch::{self, set::Set},
    completion::{self, Arena, Key, Lease, Resolved, Slots, Token},
    intrusive::avl::raw::{Entry, Tree},
};

struct Owner<'owner> {
    arena: ptr::NonNull<Arena<u64>>,
    scope: marker::PhantomData<&'owner ()>,
}

unsafe impl<'owner> completion::raw::ArenaOwner<'owner, u64> for Owner<'owner> {
    fn arena(self) -> ptr::NonNull<Arena<u64>> {
        self.arena
    }
}

impl<'owner> Owner<'owner> {
    fn new(arena: &mut Arena<u64>, _scope: &'owner ()) -> Self {
        Self {
            arena: ptr::NonNull::from(arena),
            scope: marker::PhantomData,
        }
    }
}

#[test]
fn reservation_rollback_returns_the_entry_to_its_group() {
    let mut arena = Arena::new();
    let scope = ();
    let slots = Arena::try_slots(Owner::new(&mut arena, &scope), 1).unwrap();

    let reservation = slots.reserve(11).expect("first entry");
    assert!(slots.reserve(13).is_none());
    drop(reservation);

    let reservation = slots.reserve(17).expect("rolled-back entry");
    let key = reservation.key().token();
    let lease = reservation.commit();
    drop(slots);

    assert_eq!(arena.drain().complete(key).unwrap().resolve(false), 17);
    assert_eq!(lease.complete(), 17);

    let reused = Arena::try_slots(Owner::new(&mut arena, &scope), 1).unwrap();
    let reservation = reused.reserve(19).expect("recycled entry");
    assert_ne!(reservation.key().token(), key);
    drop(reservation);
}

#[test]
fn detached_multishot_entry_waits_for_terminal_completion() {
    let mut arena = Arena::new();
    let scope = ();
    let slots = Arena::try_slots(Owner::new(&mut arena, &scope), 1).unwrap();
    let lease = slots.reserve(23).unwrap().commit();
    let key = lease.key().token();
    drop(slots);
    drop(lease);

    assert_eq!(arena.drain().complete(key).unwrap().resolve(true), 23);

    let replacement = Arena::try_slots(Owner::new(&mut arena, &scope), 1).unwrap();
    let reservation = replacement.reserve(29).expect("independent group");
    assert_ne!(reservation.key().token(), key);
    drop(reservation);
    drop(replacement);

    assert_eq!(arena.drain().complete(key).unwrap().resolve(false), 23);
}

#[test]
fn exposed_echo_round_trips_without_changing_its_identity() {
    let mut arena = Arena::new();
    let scope = ();
    let slots = Arena::try_slots(Owner::new(&mut arena, &scope), 1).unwrap();
    let lease = slots.reserve(31).unwrap().commit();
    let key = lease.key();
    let (address, serial) = key.expose();

    let returned = Token::<u64>::from_parts(address, serial).expect("valid token");
    assert_eq!((returned.address(), returned.serial()), key.expose());
    assert!(Token::<u64>::from_parts(0, serial).is_none());
    assert!(Token::<u64>::from_parts(address + 1, serial).is_none());

    drop(slots);
    assert_eq!(arena.drain().complete(returned).unwrap().resolve(false), 31);
    assert_eq!(lease.complete(), 31);
}

#[test]
fn completion_handles_have_stable_compact_layouts() {
    assert_eq!(mem::size_of::<Token<u64>>(), 2 * mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<Key<'static, u64>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Lease<'static, u64>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<Lease<'static, u64>>>(),
        2 * mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Slots<'static, u64>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Resolved<'static, u64>>(),
        2 * mem::size_of::<usize>()
    );
}

#[test]
fn batch_restore_returns_a_removed_index_without_validation() {
    let set = Set::with_capacity(4);
    assert!(set.insert(2));
    let index = set.pop().expect("index");
    unsafe { batch::raw::Set::restore_unchecked(&set, index) };
    assert_eq!(set.pop(), Some(2));
}

#[test]
fn batch_insert_and_remove_use_proven_membership() {
    let set = Set::with_capacity(4);
    assert!(set.insert(1));
    let mut drain = set.drain_batch().expect("batch");
    unsafe { batch::raw::Set::remove_unchecked(&set, 1) };
    assert_eq!(drain.next(), None);

    unsafe { batch::raw::Set::insert_unchecked(&set, 2) };
    assert!(set.contains(2));
    unsafe { batch::raw::Set::remove_unchecked(&set, 2) };
    assert!(!set.contains(2));
}

#[test]
fn linked_root_is_not_mistaken_for_an_unlinked_node() {
    let tree = Tree::new();
    let entry = Box::pin(Entry::new(1));
    unsafe { tree.insert_entry(entry.as_ref(), |_, _| false) };

    assert!(entry.as_ref().is_linked());
    assert_eq!(tree.first_entry().map(|entry| *entry.value()), Some(1));
    unsafe { tree.remove_entry(entry.as_ref()) };
    assert!(!entry.as_ref().is_linked());
    assert!(tree.first_entry().is_none());
}

#[test]
fn linked_entry_survives_owner_moves_and_mutable_pin_reborrows() {
    let tree = Tree::new();
    let entry = Box::pin(Entry::new(7));
    unsafe { tree.insert_entry(entry.as_ref(), |_, _| false) };

    let mut moved = entry;
    {
        let reborrowed = moved.as_mut();
        assert!(reborrowed.as_ref().is_linked());
    }

    assert_eq!(tree.first_entry().map(|entry| *entry.value()), Some(7));
    unsafe { tree.remove_entry(moved.as_ref()) };
    assert!(!moved.as_ref().is_linked());
}

#[test]
fn arbitrary_removal_preserves_sorted_minimum() {
    const LEN: usize = 1024;
    let tree = Tree::new();
    let mut entries: Vec<_> = (0..LEN).map(|key| Box::pin(Entry::new(key))).collect();
    let mut order: Vec<_> = (0..LEN).collect();
    for index in 0..LEN {
        let swap = (index * 37 + 17) % LEN;
        order.swap(index, swap);
    }
    for &index in &order {
        unsafe {
            tree.insert_entry(entries[index].as_ref(), |left, right| left < right);
        }
    }

    let mut expected: BTreeSet<_> = (0..LEN).collect();
    for index in (0..LEN).step_by(3) {
        unsafe { tree.remove_entry(entries[index].as_ref()) };
        expected.remove(&index);
    }
    while let Some(&key) = expected.first() {
        let first = tree
            .first_entry()
            .expect("tree must remain nonempty while expected keys remain");
        assert_eq!(*first.value(), key);
        unsafe { tree.remove_entry(first) };
        expected.remove(&key);
    }
    assert!(tree.first_entry().is_none());

    for index in (0..LEN).step_by(3) {
        unsafe {
            tree.insert_entry(entries[index].as_ref(), |left, right| left < right);
        }
    }
    for key in (0..LEN).step_by(3) {
        let first = tree
            .first_entry()
            .expect("tree must contain every key reinserted for removal");
        assert_eq!(*first.value(), key);
        unsafe { tree.remove_entry(first) };
    }
    assert!(tree.first_entry().is_none());
    entries.clear();
}

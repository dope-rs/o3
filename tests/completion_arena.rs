use std::{marker, mem, ptr};

use o3::collections::completion::{Arena, ArenaOwner, Completion, Echo, Key, Lease, Slots};

struct Owner<'owner> {
    arena: ptr::NonNull<Arena<u64>>,
    scope: marker::PhantomData<&'owner ()>,
}

// SAFETY: every test resolves all echoes before its arena and scope are
// dropped, and accesses the arena from one thread only.
unsafe impl<'owner> ArenaOwner<'owner, u64> for Owner<'owner> {
    fn arena(self) -> ptr::NonNull<Arena<u64>> {
        self.arena
    }
}

unsafe fn owner<'owner>(arena: &mut Arena<u64>, _scope: &'owner ()) -> Owner<'owner> {
    Owner {
        arena: ptr::NonNull::from(arena),
        scope: marker::PhantomData,
    }
}

#[test]
fn reservation_rollback_returns_the_entry_to_its_group() {
    let mut arena = Arena::new();
    let scope = ();
    let slots = Arena::try_slots(unsafe { owner(&mut arena, &scope) }, 1).unwrap();

    let reservation = slots.reserve(11).expect("first entry");
    assert!(slots.reserve(13).is_none());
    drop(reservation);

    let reservation = slots.reserve(17).expect("rolled-back entry");
    let key = unsafe { reservation.key().erase() };
    let lease = reservation.commit();
    drop(slots);

    assert_eq!(arena.drain().complete(key).resolve(false), 17);
    assert_eq!(lease.complete(), 17);

    let reused = Arena::try_slots(unsafe { owner(&mut arena, &scope) }, 1).unwrap();
    let reservation = reused.reserve(19).expect("recycled entry");
    assert_eq!(unsafe { reservation.key().erase() }, key);
    drop(reservation);
}

#[test]
fn detached_multishot_entry_waits_for_terminal_completion() {
    let mut arena = Arena::new();
    let scope = ();
    let slots = Arena::try_slots(unsafe { owner(&mut arena, &scope) }, 1).unwrap();
    let lease = slots.reserve(23).unwrap().commit();
    let key = unsafe { lease.key().erase() };
    drop(slots);
    drop(lease);

    assert_eq!(arena.drain().complete(key).resolve(true), 23);

    let replacement = Arena::try_slots(unsafe { owner(&mut arena, &scope) }, 1).unwrap();
    let reservation = replacement.reserve(29).expect("independent group");
    assert_ne!(unsafe { reservation.key().erase() }, key);
    drop(reservation);
    drop(replacement);

    assert_eq!(arena.drain().complete(key).resolve(false), 23);
}

#[test]
fn exposed_echo_round_trips_without_changing_its_identity() {
    let mut arena = Arena::new();
    let scope = ();
    let slots = Arena::try_slots(unsafe { owner(&mut arena, &scope) }, 1).unwrap();
    let lease = slots.reserve(31).unwrap().commit();
    let key = lease.key();
    let address = key.expose();

    let returned = unsafe { Echo::<u64>::from_exposed(address) }.expect("valid echo");
    assert_eq!(returned.expose(), key.expose());
    assert!(unsafe { Echo::<u64>::from_exposed(0) }.is_none());
    assert!(unsafe { Echo::<u64>::from_exposed(address + 1) }.is_none());

    drop(slots);
    assert_eq!(arena.drain().complete(returned).resolve(false), 31);
    assert_eq!(lease.complete(), 31);
}

#[test]
fn completion_handles_remain_pointer_sized() {
    assert_eq!(mem::size_of::<Echo<u64>>(), mem::size_of::<usize>());
    assert_eq!(mem::size_of::<Key<'static, u64>>(), mem::size_of::<usize>());
    assert_eq!(
        mem::size_of::<Lease<'static, u64>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<Lease<'static, u64>>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Slots<'static, u64>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Completion<'static, u64>>(),
        mem::size_of::<usize>()
    );
}

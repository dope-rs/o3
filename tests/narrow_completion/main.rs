mod sealed;

use std::mem;

use o3::collections::completion::narrow::{self, Arena, Echo};
pub use sealed::{Owner, allocation_count};

#[test]
fn narrow_completion_preserves_identity_lifecycle_and_operation_cost() {
    type Echo56 = Echo<u64, 24, 32>;

    assert_eq!(mem::size_of::<Echo56>(), mem::size_of::<u64>());
    assert_eq!(mem::size_of::<Option<Echo56>>(), mem::size_of::<u64>());
    assert_eq!(
        mem::size_of::<narrow::Key<'static, u64, 24, 32>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<narrow::Lease<'static, u64, 24, 32>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<Option<narrow::Lease<'static, u64, 24, 32>>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<narrow::Slots<'static, u64, 24, 32>>(),
        mem::size_of::<usize>()
    );
    assert_eq!(
        mem::size_of::<narrow::Resolved<'static, u64, 24, 32>>(),
        2 * mem::size_of::<usize>()
    );

    assert!(Echo56::from_exposed(0).is_none());
    assert!(Echo56::from_exposed(1).is_none());
    assert!(Echo56::from_exposed(1 << 56).is_none());
    assert!(Echo56::from_exposed((1 << 56) - 1).is_some());

    let mut arena = Arena::<u64, 24, 32>::new();
    let scope = ();
    let slots = Arena::try_slots(Owner::new(&mut arena, &scope), 2).unwrap();

    let rolled_back = slots.reserve(7).expect("first entry");
    let rolled_back_echo = rolled_back.key().echo();
    drop(rolled_back);
    assert!(arena.drain().complete(rolled_back_echo).is_none());

    let before = allocation_count();
    let reservation = slots.reserve(11).expect("rolled-back entry");
    let key = reservation.key();
    let exposed = key.expose();
    let returned = Echo56::from_exposed(exposed).expect("valid 56-bit echo");
    assert_eq!(returned, key.echo());
    let lease = reservation.commit();
    assert_eq!(lease.value(), 11);
    let delayed = arena.drain().complete(returned).unwrap();
    assert_eq!(
        arena
            .drain()
            .complete(returned)
            .unwrap()
            .resolve(false)
            .unwrap(),
        11
    );
    assert_eq!(lease.complete(), 11);
    let after = allocation_count();
    assert_eq!(after, before, "completion operations must not allocate");

    let replacement = slots.reserve(13).expect("reused entry");
    let replacement_echo = replacement.key().echo();
    assert_ne!(replacement_echo, returned);
    assert!(arena.drain().complete(returned).is_none());
    assert!(delayed.resolve(false).is_none());
    let replacement_lease = replacement.commit();
    drop(replacement_lease);
    assert_eq!(
        arena
            .drain()
            .complete(replacement_echo)
            .unwrap()
            .resolve(true)
            .unwrap(),
        13
    );
    assert!(arena.drain().complete(replacement_echo).is_some());
    assert_eq!(
        arena
            .drain()
            .complete(replacement_echo)
            .unwrap()
            .resolve(false)
            .unwrap(),
        13
    );
    assert!(arena.drain().complete(replacement_echo).is_none());

    let other_slots = Arena::try_slots(Owner::new(&mut arena, &scope), 1).unwrap();
    let other = other_slots.reserve(17).unwrap();
    let other_echo = other.key().echo();
    assert_ne!(other_echo.expose() as u32, replacement_echo.expose() as u32);
    let other_lease = other.commit();
    drop(other_slots);
    assert_eq!(
        arena
            .drain()
            .complete(other_echo)
            .unwrap()
            .resolve(false)
            .unwrap(),
        17
    );
    assert_eq!(other_lease.complete(), 17);

    drop(slots);

    let forged = Echo56::from_exposed((1u64 << 24) | ((1u64 << 24) - 1)).unwrap();
    assert!(arena.drain().complete(forged).is_none());

    let mut retiring = Arena::<u64, 32, 1>::new();
    let retiring_slots = Arena::try_slots(Owner::new(&mut retiring, &scope), 1).unwrap();
    let retired = retiring_slots.reserve(19).unwrap();
    let retired_echo = retired.key().echo();
    let retired_lease = retired.commit();
    assert_eq!(
        retiring
            .drain()
            .complete(retired_echo)
            .unwrap()
            .resolve(false)
            .unwrap(),
        19
    );
    assert_eq!(retired_lease.complete(), 19);
    drop(retiring_slots);

    let successor_slots = Arena::try_slots(Owner::new(&mut retiring, &scope), 1).unwrap();
    let successor = successor_slots.reserve(23).unwrap();
    let successor_echo = successor.key().echo();
    assert_ne!(successor_echo, retired_echo);
    assert!(retiring.drain().complete(retired_echo).is_none());
    drop(successor);
}

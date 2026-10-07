pub mod common;

use common::depot::queue::RingHooks;
use o3::depot::{
    self,
    queue::links,
    raw::{Hooks as _, Proof},
};

#[test]
fn ring_singleton_endpoints() {
    for slot in [1, 0] {
        let first = links::Index::try_new(0).expect("first ring index");
        let second = links::Index::try_new(1).expect("second ring index");
        let hooks = RingHooks([links::Hook::new(first), links::Hook::new(second)]);
        let ring = links::Cycle::new();
        // SAFETY: both hooks are detached and belong to this reciprocal ring store.
        unsafe {
            hooks.cycle_push(&ring, first);
            hooks.cycle_push(&ring, second);
        }
        let removed = [first, second][1 - slot];
        // SAFETY: the two-node ring is reciprocal and `removed` is its front or back.
        unsafe { hooks.cycle_unlink(&ring, removed) };
        let survivor = [first, second][slot];
        assert_eq!(ring.first(), Some(survivor));
        assert_eq!(hooks.0[slot].prev_index(), None);
        assert_eq!(hooks.0[slot].next_index(), None);
        assert!(!hooks.0[slot].is_vacant(survivor.get()));
        assert!(hooks.0[slot].is_linked(survivor.get()));
        assert!(hooks.0[slot].try_unpark(survivor).is_none());
        assert!(hooks.0[1 - slot].is_vacant(removed.get()));
        // SAFETY: the surviving head is still this ring's sole live member.
        unsafe { hooks.cycle_unlink(&ring, survivor) };
        assert!(ring.is_empty());
        assert!(hooks.0[slot].is_vacant(survivor.get()));
    }
}

#[test]
fn retains_nonempty_head() {
    let index = links::Index::try_new(0).expect("only index");
    let mut hooks = RingHooks([links::Hook::new(index)]);
    let list = links::List::new();
    // SAFETY: the detached hook and empty list belong to this exact one-slot plane.
    unsafe { hooks.link_raw(&list, index) };
    assert_eq!(list.front(), Some(index.get()));
    // SAFETY: this original list still owns its initialized head without intervening mutation.
    let head = links::List::front_from(unsafe { Proof::new(&list) });
    assert_eq!(head, index);
    // SAFETY: the retained head identifies the sole member in the same plane.
    unsafe { hooks.unlink_raw(&list, head) };
    assert!(list.is_empty());
}

#[test]
fn try_unpark_once() {
    let index = links::Index::try_new(0).expect("parked ring index");
    let hooks = RingHooks([
        links::Hook::new(index),
        links::Hook::new(links::Index::try_new(1).expect("second ring index")),
    ]);
    let chain = links::Chain::<()>::new();
    // SAFETY: the hook is detached and belongs to this chain's store.
    unsafe { hooks.chain_push(&chain, index) };
    // SAFETY: the hook is the sole reciprocal member of this chain.
    let vacant = unsafe { hooks.chain_unlink(&chain, index) };
    vacant.park();
    assert!(hooks.0[0].try_unpark(index).is_some());
    assert!(hooks.0[0].try_unpark(index).is_none());
}

#[test]
fn ring_progress() {
    let keys = [
        links::Index::try_new(0).expect("first ring index"),
        links::Index::try_new(1).expect("middle ring index"),
        links::Index::try_new(2).expect("last ring index"),
    ];
    let hooks = RingHooks([
        links::Hook::new(keys[0]),
        links::Hook::new(keys[1]),
        links::Hook::new(keys[2]),
    ]);
    let ring = links::Cycle::new();
    // SAFETY: all hooks are detached and belong to this reciprocal ring store.
    unsafe {
        hooks.cycle_push(&ring, keys[0]);
        hooks.cycle_push(&ring, keys[1]);
        hooks.cycle_push(&ring, keys[2]);
    }
    assert_eq!(ring.first(), Some(keys[0]));
    // SAFETY: the three-node ring is reciprocal and `keys[0]` is its front.
    unsafe { hooks.cycle_unlink(&ring, keys[0]) };
    assert_eq!(ring.first(), Some(keys[1]));
    assert_eq!(hooks.0[1].next_index(), Some(keys[2]));
    assert_eq!(hooks.0[2].prev_index(), Some(keys[1]));
    // SAFETY: the two-node ring is reciprocal and `keys[2]` is its back.
    unsafe { hooks.cycle_unlink(&ring, keys[2]) };
    assert_eq!(ring.first(), Some(keys[1]));
    assert_eq!(hooks.0[1].prev_index(), None);
    assert_eq!(hooks.0[1].next_index(), None);
    // SAFETY: the ring is a valid singleton with `keys[1]` as its head.
    unsafe { hooks.cycle_step(&ring, keys[1]) };
    assert_eq!(ring.first(), Some(keys[1]));
    // SAFETY: the singleton head is the only live member.
    unsafe { hooks.cycle_unlink(&ring, keys[1]) };
    assert!(ring.is_empty());
}

#[test]
fn parks_then_becomes_idle() {
    let index = links::Index::try_new(3).expect("index");
    let hook = links::Hook::new(index);
    // SAFETY: this fresh local hook has no topology or other detached capability.
    links::Detached::from_source(unsafe { depot::raw::Proof::new((&hook, index)) }).park();
    let detached = hook.try_unpark(index).expect("parked");
    assert!(hook.try_unpark(index).is_none());
    detached.idle();
    assert!(hook.is_vacant(index.get()));
}

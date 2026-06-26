use o3::slab::Slab;

#[test]
fn generational_reuse_and_capacity() {
    let mut s: Slab<&str> = Slab::new(3);
    let a = s.alloc("a").unwrap();
    let b = s.alloc("b").unwrap();
    let c = s.alloc("c").unwrap();
    assert_eq!(s.len(), 3);
    assert!(s.alloc("d").is_none());
    assert_eq!(s.get(a), Some(&"a"));

    // Removing then reallocating reuses the slot index with a bumped
    // generation, so the stale handle no longer resolves.
    assert!(s.remove(b));
    let b2 = s.alloc("b2").unwrap();
    assert_eq!(b.slot(), b2.slot());
    assert_ne!(b.generation(), b2.generation());
    assert_eq!(s.get(b), None);
    assert_eq!(s.get(b2), Some(&"b2"));
    assert!(!s.remove(b));

    *s.get_mut(c).unwrap() = "C";
    assert_eq!(s.get(c), Some(&"C"));
}

#[test]
fn reservation_and_place_at() {
    let mut s: Slab<u32> = Slab::new(8);

    // A filled reservation commits; a dropped one releases its slot.
    let r = s.reserve().unwrap();
    let id = r.fill(7);
    assert_eq!(s.get(id), Some(&7));
    let r = s.reserve().unwrap();
    drop(r);
    assert_eq!(s.len(), 1);

    // place_at fills a caller-chosen index directly.
    let p = s.place_at(5, |id| id.slot());
    assert_eq!(p.slot(), 5);
    assert_eq!(s.get(p), Some(&5));
    assert_eq!(s.at_index(5).map(|(v, _)| *v), Some(5));
    // Slot 1 held the dropped reservation, so it reads as free.
    assert_eq!(s.at_index(1), None);
}

use o3::{
    buffer::{PrefixConsumer as _, resident, view},
    cell::region,
};

#[test]
fn plain_snapshot_keeps_shared_ranges_stable_across_reuse() {
    let mut snapshot = view::Snapshot::<16>::new();
    snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
    let retained = snapshot.snapshot().unwrap();
    snapshot.try_consume_prefix(5).unwrap().commit();
    snapshot.try_extend(&[6, 7, 8, 9]).unwrap();
    assert_eq!(retained.as_slice(), &[1, 2, 3, 4, 5]);
    assert_eq!(snapshot.snapshot().unwrap().as_slice(), &[6, 7, 8, 9]);
}

#[test]
fn shared_views_hold_the_original_resident_charge() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(16, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(budget.resident(), 8);

        let retained = snapshot.snapshot().unwrap();
        assert_eq!(retained.resident_bytes(), 8);
        snapshot.try_consume_prefix(5).unwrap().commit();
        snapshot.try_extend(&[6, 7, 8, 9]).unwrap();
        assert_eq!(budget.resident(), 16);

        drop(snapshot);
        assert_eq!(budget.resident(), 8);
        drop(retained);
        assert_eq!(budget.resident(), 0);
    });
}

#[test]
fn shared_replacement_fails_before_exceeding_the_budget() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(8, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        let retained = snapshot.snapshot().unwrap();
        snapshot.try_consume_prefix(5).unwrap().commit();

        assert!(snapshot.try_extend(&[6, 7, 8, 9]).is_err());
        assert_eq!(budget.resident(), 8);

        drop(snapshot);
        assert_eq!(budget.resident(), 8);
        drop(retained);
        assert_eq!(budget.resident(), 0);
    });
}

#[test]
fn unique_growth_charges_only_the_capacity_delta() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(16, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        snapshot.try_extend(&[6, 7, 8, 9, 10]).unwrap();
        assert_eq!(budget.resident(), 16);
        drop(snapshot);
        assert_eq!(budget.resident(), 0);
    });
}

#[test]
fn explicit_charge_is_shared_and_released_once() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(16, &token);
        let charge = budget.try_charge(12).unwrap();
        assert_eq!(budget.resident(), 12);
        drop(charge);
        assert_eq!(budget.resident(), 0);
    });
}

#[test]
fn empty_snapshot_releases_its_high_water_owner() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(16, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        let retained = snapshot.snapshot().unwrap();
        snapshot.try_consume_prefix(5).unwrap().commit();
        snapshot.release_empty();
        assert_eq!(budget.resident(), 8);
        drop(retained);
        assert_eq!(budget.resident(), 0);
        snapshot.try_extend(&[6, 7, 8, 9]).unwrap();
        assert_eq!(budget.resident(), 4);
    });
}

#[test]
fn growth_uses_the_exact_remaining_budget_after_geometric_reservation_fails() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(10, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        snapshot.try_extend(&[0; 9]).unwrap();
        assert_eq!(budget.resident(), 9);
    });
}

#[test]
fn failed_unique_growth_preserves_the_unparsed_range() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(8, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        snapshot.try_consume_prefix(2).unwrap().commit();
        assert!(snapshot.try_extend(&[6, 7, 8, 9, 10, 11]).is_err());
        assert_eq!(snapshot.snapshot().unwrap().as_slice(), &[3, 4, 5]);
    });
}

#[test]
fn snapshot_owns_the_budget_core_after_the_handle_drops() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(8, &token);
        let mut snapshot = resident::Snapshot::<64>::new(&budget);
        drop(budget);
        snapshot.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        assert_eq!(snapshot.snapshot().unwrap().as_slice(), &[1, 2, 3, 4, 5]);
    });
}

#[test]
fn one_budget_caps_multiple_snapshots() {
    region::Token::scope(|token| {
        let budget = resident::Budget::new(12, &token);
        let mut first = resident::Snapshot::<64>::new(&budget);
        let mut second = resident::Snapshot::<64>::new(&budget);
        first.try_extend(&[1, 2, 3, 4, 5]).unwrap();
        assert!(second.try_extend(&[6, 7, 8, 9, 10]).is_err());
        assert_eq!(budget.resident(), 8);
    });
}

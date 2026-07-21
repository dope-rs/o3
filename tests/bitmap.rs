use o3::collections::CellBitmap;

#[test]
fn rotates_and_wraps() {
    let bits = CellBitmap::with_capacity(130);
    assert!(bits.insert(65));
    assert!(bits.insert(2));
    assert!(!bits.insert(65));
    assert_eq!(bits.pop_next(), Some(2));
    assert!(bits.insert(1));
    assert_eq!(bits.pop_next(), Some(65));
    assert_eq!(bits.pop_next(), Some(1));
    assert!(bits.is_empty());
}

#[test]
fn filled_masks_tail() {
    let free = CellBitmap::filled(65);
    assert_eq!(free.len(), 65);
    for expected in 0..65 {
        assert_eq!(free.pop_next(), Some(expected));
    }
    assert_eq!(free.pop_next(), None);
}

#[test]
fn sparse_successor_lookup_crosses_summary_levels() {
    let bitmap = CellBitmap::with_capacity(1 << 20);
    assert!(bitmap.insert(900_001));
    assert!(bitmap.insert(17));
    assert_eq!(bitmap.pop_next(), Some(17));
    assert_eq!(bitmap.pop_next(), Some(900_001));
    assert!(bitmap.is_empty());

    bitmap.grow_to(1 << 21);
    assert!(bitmap.insert(1_900_003));
    assert_eq!(bitmap.pop_next(), Some(1_900_003));
}

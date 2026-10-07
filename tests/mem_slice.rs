use o3::mem::slice::Ext as _;

#[test]
fn splits_while() {
    let source = [0, 1, 2, 3];
    for limit in 0..=source.len() {
        let mut calls = 0;
        let (left, right) = source.split_while(|&value| {
            calls += 1;
            value < limit
        });
        assert_eq!(calls, (limit + 1).min(source.len()));
        assert_eq!(left, &source[..limit]);
        assert_eq!(right, &source[limit..]);
        assert_eq!(left.as_ptr(), source.as_ptr());
        assert_eq!(right.as_ptr(), source[limit..].as_ptr());
    }
    let mut called = false;
    let (left, right) = source[..0].split_while(|_| {
        called = true;
        false
    });
    assert!(!called && left.is_empty() && right.is_empty());
}

#[test]
fn copies_tail() {
    let source = [0, 1, 2, 3, 4, 5, 6, 7];
    for start in 0..=source.len() {
        let mut target = source;
        assert_eq!(target.copy_tail(start).expect("suffix"), &source[start..]);
    }
    let mut target = source;
    assert!(target.copy_tail(source.len() + 1).is_none());
    assert!(target.copy_tail(usize::MAX).is_none());
    assert_eq!(target, source);
    assert_eq!([(); 4].copy_tail(2).expect("ZST suffix").len(), 2);
}

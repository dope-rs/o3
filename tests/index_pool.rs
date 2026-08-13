use std::mem;

use o3::collections::fixed::index::Pool;

#[test]
fn detached_indices_reuse_in_constant_time() {
    let mut pool = Pool::try_with_capacity(3).unwrap();

    assert_eq!(pool.capacity(), 3);
    assert_eq!(pool.available(), 3);
    assert_eq!(pool.take(), Some(0));
    let second = pool.take().unwrap();
    assert_eq!(second, 1);
    assert_eq!(pool.take(), Some(2));
    assert!(pool.is_exhausted());
    assert_eq!(pool.take(), None);

    assert!(pool.release(second));
    assert_eq!(pool.available(), 1);
    assert_eq!(pool.take(), Some(second));
}

#[test]
fn zero_capacity_is_permanently_exhausted() {
    let mut pool = Pool::try_with_capacity(0).unwrap();

    assert_eq!(pool.capacity(), 0);
    assert_eq!(pool.available(), 0);
    assert!(pool.is_exhausted());
    assert_eq!(pool.take(), None);
}

#[test]
fn pool_stores_only_the_links_and_two_indices() {
    assert_eq!(
        mem::size_of::<Pool>(),
        mem::size_of::<(Box<[u32]>, [u32; 2])>()
    );
}

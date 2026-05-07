use std::ptr::NonNull;

use o3::cell::LateBound;

#[test]
fn unbound_then_bind_then_rebind() {
    let mut a = 1u32;
    let mut b = 2u32;

    let h1 = LateBound::<u32>::unbound();
    assert!(!h1.is_bound());
    assert!(h1.as_ptr().is_none());

    let h2 = h1.clone();

    h1.bind(NonNull::from(&mut a));
    assert!(h2.is_bound());
    unsafe {
        assert_eq!(*h2.as_ref(), 1);
        *h1.as_mut() = 10;
    }
    assert_eq!(a, 10);

    h2.bind(NonNull::from(&mut b));
    unsafe {
        assert_eq!(*h1.as_ref(), 2);
    }

    h1.unbind();
    assert!(!h2.is_bound());
}

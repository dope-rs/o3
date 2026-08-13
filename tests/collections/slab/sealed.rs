use o3::collections::slab::{Capacity, Exclusive, raw::Recycling as _};

#[test]
fn private_generations_can_wrap_for_a_wider_identity_wrapper() {
    let capacity = Capacity::new(1);
    // SAFETY: physical handles remain private and are never stale-identity authority.
    let mut slab = unsafe {
        Exclusive::<u32, (), 2, true>::try_with_capacity_recycling(capacity)
            .expect("recycling slab")
    };

    let first = slab.insert(1).expect("generation one");
    assert_eq!(slab.remove(first), Some(1));
    let second = slab.insert(2).expect("generation two");
    assert_eq!(slab.remove(second), Some(2));
    let wrapped = slab.insert(3).expect("wrapped generation one");
    assert_eq!(wrapped.generation().get(), 1);
    assert_eq!(slab.remove(wrapped), Some(3));
}

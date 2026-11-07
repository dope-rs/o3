use std::pin::Pin;

use o3::base::cell::{brand, local};

#[test]
fn borrows_branded_regions() {
    brand::scope_pinned(Pin::new(&mut ()), |mut token, (mut io, mut state), _| {
        let value = token.bind(1);
        *token.pin_mut(Pin::new(&value)).get_mut() = 2;
        assert_eq!(*token.borrow_pin(Pin::new(&value)), 2);
        let buffer = io.bind(1_u64);
        let mut counter = state.proof().bind(2_u64);
        let count = state.borrow_mut(&counter);
        let bytes = io.borrow_mut(&buffer);
        *bytes += *count;
        *count += *bytes;
        assert_eq!((*io.borrow(&buffer), *state.borrow(&counter)), (3, 5));
        buffer.write(local::Access::new(&mut io), |value| *value += 2);
        let mut access = local::Access::new(&mut state);
        counter.write(access.reborrow(), |value| *value += 3);
        assert_eq!(counter.read(access.read(), |value| *value), 8);
        access.with_region(|token| *token.borrow_mut(&counter) += 1);
        assert_eq!(buffer.into_inner(), 5);
        assert_eq!(*counter.get_mut(), 9);
    });
}

use std::pin::Pin;

use o3::base::{
    cell::{brand, local},
    marker::ThreadBound,
};

#[test]
fn keeps_marker_layout() {
    assert_eq!(std::mem::size_of::<ThreadBound>(), 0);
    assert_eq!(std::mem::align_of::<ThreadBound>(), 1);
}

#[test]
fn borrows_branded_regions() {
    brand::Token::scope_pinned(Pin::new(&mut ()), |mut token, (mut io, mut state), _| {
        let value = token.bind(1);
        *token.pin_mut(Pin::new(&value)).get_mut() = 2;
        assert_eq!(*token.pin_mut(Pin::new(&value)).get_mut(), 2);
        let buffer = io.bind(1_u64);
        let counter = state.bind(2_u64);
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
        assert_eq!(*io.borrow(&buffer), 5);
        assert_eq!(*state.borrow(&counter), 9);
    });
}

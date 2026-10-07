use o3::buffer::pool::{self, Fixed, Layout};

#[test]
fn layouts_match() {
    const INPUTS: [(usize, usize); 7] = [
        (0, 1),
        (5, 17),
        (u32::MAX as usize, 1),
        (usize::MAX, 0),
        (1, 0),
        (0, usize::MAX),
        (u32::MAX as usize, u32::MAX as usize),
    ];
    const EXPECTED: [Result<Layout, pool::LayoutError>; INPUTS.len()] = {
        let mut outcomes = [Err(pool::LayoutError::ZeroCapacity); INPUTS.len()];
        let mut index = 0;
        while index < INPUTS.len() {
            let (slots, capacity) = INPUTS[index];
            outcomes[index] = Layout::new(slots, capacity);
            index += 1;
        }
        outcomes
    };
    assert_eq!(EXPECTED[3], Err(pool::LayoutError::SlotsTooLarge));
    assert_eq!(EXPECTED[4], Err(pool::LayoutError::ZeroCapacity));
    assert_eq!(EXPECTED[5], Err(pool::LayoutError::CapacityTooLarge));
    for ((slots, capacity), expected) in INPUTS.into_iter().zip(EXPECTED) {
        assert_eq!(Layout::new(std::hint::black_box(slots), capacity), expected);
    }
    const STATIC: Layout = Layout::constant::<5, 17>();
    const FIXED: Fixed<5, 17> = Fixed::new();
    assert_eq!(Ok(STATIC), EXPECTED[1]);
    assert_eq!(Layout::from(FIXED), STATIC);
    assert_eq!(
        Layout::from(pool::Width::<17>::new(5).expect("five fixed-width slots")),
        STATIC
    );
}

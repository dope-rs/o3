use std::time::Duration;

use o3::base::time::{Add, Millis, Nanos, Positive, Sub, Unix};

#[test]
fn checks_unix_arithmetic() {
    let step = Positive::from_nanos::<500>().get();
    assert!(Unix::<Nanos>::new(1_500).try_add(step) == Ok(Unix::<Nanos>::new(2_000)));
    assert_eq!(
        Unix::<Nanos>::new(500).try_sub(step),
        Ok(Unix::<Nanos>::ZERO)
    );
    let at = Unix::<Nanos>::new(u64::MAX);
    let duration = Duration::from_nanos(1);
    assert!(at.try_add(duration) == Err(Add::Range { at, duration }));
    let at = Unix::<Nanos>::ZERO;
    assert_eq!(at.try_sub(duration), Err(Sub::Range { at, duration }));
    for ticks in [0, u64::MAX] {
        let end = Unix::<Nanos>::new(ticks);
        let duration = Duration::from_nanos(ticks);
        assert_eq!(at.try_add(duration), Ok(end));
        assert_eq!(end.try_sub(duration), Ok(at));
    }
    for duration in [
        Duration::from_nanos(u64::MAX) + Duration::from_nanos(1),
        Duration::MAX,
    ] {
        assert_eq!(at.try_add(duration), Err(Add::Scale { at, duration }));
        assert_eq!(at.try_sub(duration), Err(Sub::Scale { at, duration }));
    }
    assert_eq!(
        Unix::<Nanos>::new(1_999_999_999).floor_millis().ticks(),
        1_999
    );
    assert_eq!(Unix::<Millis>::new(1_999).floor_secs().ticks(), 1);
}

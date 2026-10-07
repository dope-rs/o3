use std::time::Duration;

use o3::base::time::DurationExt as _;

#[test]
fn averages_full_domain() {
    let values = [
        Duration::ZERO,
        Duration::from_nanos(1),
        Duration::from_nanos(999_999_999),
        Duration::from_secs(1),
        Duration::from_secs(u64::MAX),
        Duration::MAX,
    ];
    for prior in values {
        for sample in values {
            for (n, result) in [
                (2, prior.average::<2>(sample)),
                (4, prior.average::<4>(sample)),
                (8, prior.average::<8>(sample)),
            ] {
                let expected = ((n - 1) * prior.as_nanos() + sample.as_nanos()) / n;
                assert_eq!(result.as_nanos(), expected, "{prior:?}, {sample:?}, {n}");
            }
        }
    }
}

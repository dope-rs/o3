use o3::base::time::DurationExt as _;
use std::time::Duration;

fn main() {
    let _ = Duration::ZERO.average::<3>(Duration::MAX);
}

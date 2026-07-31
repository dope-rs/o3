use o3::buffer::{Initialized, SharedPool};

fn main() {
    let pool = SharedPool::<Initialized>::try_new(1, 8).unwrap();
    let mut lease = pool.try_acquire().unwrap();
    let _ = lease.spare_writer();
}
